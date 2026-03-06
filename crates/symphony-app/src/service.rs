use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use axum::extract::{Path as AxumPath, Request as AxumRequest, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use clap::Parser;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{debug, error, info, warn};

use symphony_core::{
    DispatchPolicy, RetryPlan, WorkerExitReason, apply_absolute_token_totals, apply_worker_exit,
    available_slots, build_runtime_snapshot, build_service_config, initial_runtime_state,
    parse_running_state_count, parse_state_set, parse_workflow, register_running_issue,
    render_issue_prompt, should_dispatch, sort_for_dispatch, validate_dispatch_config,
};
use symphony_domain::{
    BlockerRef, Issue, IssueId, IssueIdentifier, RetryEntry, ServiceConfig, SessionId, ThreadId,
    TrackerConfig, TurnCount, TurnId, WorkflowDefinition, parse_issue_id, parse_issue_identifier,
    parse_issue_state, parse_issue_title, parse_label, sanitize_workspace_key,
};

use crate::ui::{render_dashboard, site_pkg_dir, site_root};

const DEFAULT_PROMPT: &str = "You are working on an issue from Linear.";
const CONTINUATION_PROMPT: &str =
    "Continue from prior thread context and make the next concrete progress step on this issue.";
const MAX_PROTOCOL_LINE_BYTES: usize = 10_485_760;
const RECENT_EVENT_LIMIT: usize = 25;

#[derive(Debug, Clone, Parser)]
#[command(
    name = "symphony",
    version,
    about = "Symphony issue orchestration service"
)]
struct CliArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: Option<PathBuf>,

    #[arg(long, value_name = "PORT")]
    port: Option<u16>,

    #[arg(long)]
    once: bool,
}

#[derive(Debug, Error)]
pub(crate) enum AppError {
    #[error("missing_workflow_file")]
    MissingWorkflowFile,
    #[error("workflow_parse_error: {0}")]
    WorkflowParseError(String),
    #[error("config_error: {0}")]
    ConfigError(String),
    #[error("linear_api_request: {0}")]
    LinearApiRequest(String),
    #[error("linear_api_status: {0}")]
    LinearApiStatus(String),
    #[error("linear_graphql_errors: {0}")]
    LinearGraphqlErrors(String),
    #[error("workspace_error: {0}")]
    WorkspaceError(String),
    #[error("codex_error: {0}")]
    CodexError(String),
    #[error("protocol_error: {0}")]
    ProtocolError(String),
    #[error("http_error: {0}")]
    HttpError(String),
}

#[derive(Debug, Clone)]
struct LoadedWorkflow {
    workflow_path: PathBuf,
    workflow: WorkflowDefinition,
    config: ServiceConfig,
}

#[derive(Debug, Clone)]
struct Workspace {
    path: PathBuf,
    created_now: bool,
}

#[derive(Debug, Clone)]
struct RecentEvent {
    at: DateTime<Utc>,
    event: String,
    message: Option<String>,
}

#[derive(Debug, Clone)]
enum ServiceEvent {
    WorkflowReloaded {
        loaded: Box<LoadedWorkflow>,
    },
    WorkerExited {
        issue_id: IssueId,
        reason: WorkerExitReason,
        runtime_seconds: u64,
    },
    CodexSessionStarted {
        issue_id: IssueId,
        session_id: String,
        thread_id: String,
        turn_id: String,
        pid: Option<String>,
    },
    CodexTokenTotals {
        issue_id: IssueId,
        input_tokens: u64,
        output_tokens: u64,
        total_tokens: u64,
    },
    CodexRateLimits {
        payload: Value,
    },
    CodexActivity {
        issue_id: IssueId,
        event: String,
        message: Option<String>,
        at: DateTime<Utc>,
    },
    RetryDue {
        issue_id: IssueId,
    },
    ForceRefresh,
}

#[derive(Clone)]
struct SharedAppState {
    runtime: Arc<Mutex<symphony_domain::RuntimeState>>,
    workflow: Arc<RwLock<LoadedWorkflow>>,
    recent_events: Arc<Mutex<HashMap<IssueId, VecDeque<RecentEvent>>>>,
    event_tx: UnboundedSender<ServiceEvent>,
}

#[derive(Serialize)]
struct ApiErrorEnvelope {
    error: ApiErrorBody,
}

#[derive(Serialize)]
struct ApiErrorBody {
    code: &'static str,
    message: String,
}

#[derive(Serialize)]
struct RefreshResponse {
    queued: bool,
    coalesced: bool,
    requested_at: DateTime<Utc>,
    operations: Vec<&'static str>,
}

#[derive(Serialize)]
struct IssueDebugResponse {
    issue_identifier: String,
    issue_id: String,
    status: String,
    workspace: IssueWorkspace,
    attempts: IssueAttempts,
    running: Option<IssueRunning>,
    retry: Option<IssueRetry>,
    recent_events: Vec<IssueEvent>,
    last_error: Option<String>,
}

#[derive(Serialize)]
struct IssueWorkspace {
    path: String,
}

#[derive(Serialize)]
struct IssueAttempts {
    restart_count: u32,
    current_retry_attempt: u32,
}

#[derive(Serialize)]
struct IssueRunning {
    session_id: Option<String>,
    turn_count: u32,
    state: String,
    started_at: DateTime<Utc>,
    last_event: Option<String>,
    last_message: Option<String>,
    last_event_at: Option<DateTime<Utc>>,
    tokens: IssueTokens,
}

#[derive(Serialize)]
struct IssueTokens {
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
}

#[derive(Serialize)]
struct IssueRetry {
    attempt: u32,
    due_at: DateTime<Utc>,
    error: Option<String>,
}

#[derive(Serialize)]
struct IssueEvent {
    at: DateTime<Utc>,
    event: String,
    message: Option<String>,
}

#[derive(Clone)]
struct WorkerLaunchConfig {
    issue: Issue,
    attempt: Option<u32>,
    workflow: WorkflowDefinition,
    config: ServiceConfig,
}

struct Orchestrator {
    shared: SharedAppState,
    tracker: Arc<dyn IssueTracker>,
    event_rx: UnboundedReceiver<ServiceEvent>,
    worker_handles: HashMap<IssueId, JoinHandle<()>>,
    retry_handles: HashMap<IssueId, JoinHandle<()>>,
    workflow_watcher: RecommendedWatcher,
    once: bool,
}

#[async_trait]
trait IssueTracker: Send + Sync {
    async fn fetch_candidate_issues(
        &self,
        active_states: &[String],
    ) -> Result<Vec<Issue>, AppError>;
    async fn fetch_issues_by_states(&self, states: &[String]) -> Result<Vec<Issue>, AppError>;
    async fn fetch_issue_states_by_ids(&self, issue_ids: &[String])
    -> Result<Vec<Issue>, AppError>;
    async fn execute_raw_graphql(
        &self,
        query: &str,
        variables: Option<serde_json::Map<String, Value>>,
    ) -> Result<Value, AppError>;
}

#[derive(Clone)]
struct LinearTracker {
    endpoint: String,
    api_key: String,
    project_slug: String,
    http_client: reqwest::Client,
}

#[derive(Clone)]
struct WorkspaceManager {
    root: PathBuf,
}

struct CodexIo<'a> {
    issue_id: &'a IssueId,
    tracker: Arc<dyn IssueTracker>,
    event_tx: UnboundedSender<ServiceEvent>,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    read_timeout_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TurnOutcome {
    Completed,
    Failed,
    Cancelled,
}

pub async fn run() -> anyhow::Result<()> {
    configure_logging();
    let _ = any_spawner::Executor::init_tokio();

    let args = CliArgs::parse();
    let workflow_path = discover_workflow_path(args.workflow_path)?;
    let loaded = load_workflow_file(&workflow_path)?;
    validate_dispatch_config(&loaded.config)
        .map_err(|error| AppError::ConfigError(error.to_string()))?;

    let runtime = Arc::new(Mutex::new(initial_runtime_state(
        loaded.config.polling.interval_ms.value(),
        loaded.config.agent.max_concurrent_agents.value(),
    )));
    let workflow = Arc::new(RwLock::new(loaded.clone()));
    let recent_events = Arc::new(Mutex::new(HashMap::<IssueId, VecDeque<RecentEvent>>::new()));

    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let shared = SharedAppState {
        runtime: Arc::clone(&runtime),
        workflow: Arc::clone(&workflow),
        recent_events,
        event_tx: event_tx.clone(),
    };

    let tracker: Arc<dyn IssueTracker> = Arc::new(LinearTracker::new(&loaded.config.tracker)?);
    let workspace_manager = WorkspaceManager::new(&loaded.config)?;
    if let Err(error) =
        startup_terminal_workspace_cleanup(&*tracker, &workspace_manager, &loaded.config).await
    {
        warn!(reason = %error, "startup_terminal_workspace_cleanup failed; continuing startup");
    }

    let watcher = spawn_workflow_watcher(loaded.workflow_path.clone(), event_tx.clone())?;

    let port = args
        .port
        .or(loaded.config.server.as_ref().map(|value| value.port));
    if !args.once
        && let Some(port) = port
    {
        let server_state = shared.clone();
        tokio::spawn(async move {
            if let Err(error) = run_http_server(server_state, port).await {
                error!(reason = %error, "http server failed");
            }
        });
    }

    let orchestrator = Orchestrator {
        shared,
        tracker,
        event_rx,
        worker_handles: HashMap::new(),
        retry_handles: HashMap::new(),
        workflow_watcher: watcher,
        once: args.once,
    };

    orchestrator.run().await.map_err(anyhow::Error::from)
}

impl Orchestrator {
    async fn run(mut self) -> Result<(), AppError> {
        self.process_tick().await;
        if self.once {
            return Ok(());
        }

        let mut interval_ms = self.poll_interval_ms().await;
        let mut interval = tokio::time::interval(Duration::from_millis(interval_ms));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let _ = interval.tick().await;

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.process_tick().await;
                }
                maybe_event = self.event_rx.recv() => {
                    let Some(event) = maybe_event else {
                        break;
                    };
                    self.handle_event(event).await;
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("received ctrl-c; shutting down");
                    self.shutdown_workers().await;
                    break;
                }
            }

            let updated_interval_ms = self.poll_interval_ms().await;
            if updated_interval_ms != interval_ms {
                interval_ms = updated_interval_ms;
                interval = tokio::time::interval(Duration::from_millis(interval_ms));
                interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
                let _ = interval.tick().await;
            }
        }

        Ok(())
    }

    async fn poll_interval_ms(&self) -> u64 {
        self.shared
            .workflow
            .read()
            .await
            .config
            .polling
            .interval_ms
            .value()
    }

    async fn process_tick(&mut self) {
        self.reconcile_running_issues().await;

        let loaded = self.shared.workflow.read().await.clone();
        if let Err(error) = validate_dispatch_config(&loaded.config) {
            error!(reason = %error, "dispatch validation failed; skipping dispatch");
            return;
        }

        let active_states = loaded
            .config
            .tracker
            .active_states
            .iter()
            .map(|state| state.value().to_string())
            .collect::<Vec<_>>();

        let candidates = match self.tracker.fetch_candidate_issues(&active_states).await {
            Ok(issues) => issues,
            Err(error) => {
                error!(reason = %error, "candidate issue fetch failed");
                return;
            }
        };

        let policy = DispatchPolicy {
            active_states: parse_state_set(&loaded.config.tracker.active_states),
            terminal_states: parse_state_set(&loaded.config.tracker.terminal_states),
            agent: loaded.config.agent.clone(),
        };

        for issue in sort_for_dispatch(&candidates) {
            let dispatchable = {
                let runtime = self.shared.runtime.lock().await;
                should_dispatch(&issue, &runtime, &policy)
            };

            if !dispatchable {
                continue;
            }

            self.dispatch_issue(issue, None, false).await;
        }
    }

    async fn dispatch_issue(
        &mut self,
        issue: Issue,
        attempt: Option<u32>,
        allow_existing_claim: bool,
    ) {
        let loaded = self.shared.workflow.read().await.clone();
        let now = Utc::now();

        {
            let policy = DispatchPolicy {
                active_states: parse_state_set(&loaded.config.tracker.active_states),
                terminal_states: parse_state_set(&loaded.config.tracker.terminal_states),
                agent: loaded.config.agent.clone(),
            };
            let mut runtime = self.shared.runtime.lock().await;
            let dispatchable = if allow_existing_claim {
                let mut projected = runtime.clone();
                projected.claimed.remove(&issue.id);
                should_dispatch(&issue, &projected, &policy)
            } else {
                should_dispatch(&issue, &runtime, &policy)
            };

            if !dispatchable {
                return;
            }

            register_running_issue(&mut runtime, issue.clone(), attempt, now);
        }

        let issue_id = issue.id.clone();
        let task_issue_id = issue_id.clone();
        let tracker = Arc::clone(&self.tracker);
        let event_tx = self.shared.event_tx.clone();
        let launch = WorkerLaunchConfig {
            issue,
            attempt,
            workflow: loaded.workflow,
            config: loaded.config,
        };

        let handle = tokio::spawn(async move {
            let started = std::time::Instant::now();
            let reason = run_agent_attempt(launch, tracker, event_tx.clone()).await;
            let runtime_seconds = started.elapsed().as_secs();
            let _ = event_tx.send(ServiceEvent::WorkerExited {
                issue_id: task_issue_id,
                reason,
                runtime_seconds,
            });
        });

        self.worker_handles.insert(issue_id, handle);
    }

    async fn handle_event(&mut self, event: ServiceEvent) {
        match event {
            ServiceEvent::WorkflowReloaded { loaded } => {
                info!(path = %loaded.workflow_path.display(), "workflow reloaded");
                let loaded = *loaded;
                let tracker = LinearTracker::new(&loaded.config.tracker);
                match tracker {
                    Ok(next_tracker) => {
                        *self.shared.workflow.write().await = loaded;
                        self.tracker = Arc::new(next_tracker);
                        let mut runtime = self.shared.runtime.lock().await;
                        runtime.poll_interval_ms = self
                            .shared
                            .workflow
                            .read()
                            .await
                            .config
                            .polling
                            .interval_ms
                            .value();
                        runtime.max_concurrent_agents = self
                            .shared
                            .workflow
                            .read()
                            .await
                            .config
                            .agent
                            .max_concurrent_agents
                            .value();
                    }
                    Err(error) => {
                        error!(reason = %error, "workflow reload invalid; keeping previous config");
                    }
                }
            }
            ServiceEvent::WorkerExited {
                issue_id,
                reason,
                runtime_seconds,
            } => {
                self.worker_handles.remove(&issue_id);
                let max_backoff_ms = self
                    .shared
                    .workflow
                    .read()
                    .await
                    .config
                    .agent
                    .max_retry_backoff_ms
                    .value();
                let now_ms = epoch_millis();

                let retry = {
                    let mut runtime = self.shared.runtime.lock().await;
                    runtime.codex_totals.seconds_running = runtime
                        .codex_totals
                        .seconds_running
                        .saturating_add(runtime_seconds);
                    apply_worker_exit(&mut runtime, &issue_id, reason, now_ms, max_backoff_ms)
                };

                if let Some(retry_entry) = retry {
                    self.schedule_retry_timer(&retry_entry).await;
                }
            }
            ServiceEvent::CodexSessionStarted {
                issue_id,
                session_id,
                thread_id,
                turn_id,
                pid,
            } => {
                let mut runtime = self.shared.runtime.lock().await;
                if let Some(entry) = runtime.running.get_mut(&issue_id) {
                    entry.live_session.session_id = SessionId::try_new(session_id).ok();
                    entry.live_session.thread_id = ThreadId::try_new(thread_id).ok();
                    entry.live_session.turn_id = TurnId::try_new(turn_id).ok();
                    entry.live_session.codex_app_server_pid = pid;
                    entry.live_session.turn_count =
                        increment_turn_count(entry.live_session.turn_count);
                }
            }
            ServiceEvent::CodexTokenTotals {
                issue_id,
                input_tokens,
                output_tokens,
                total_tokens,
            } => {
                let mut runtime = self.shared.runtime.lock().await;
                apply_absolute_token_totals(
                    &mut runtime,
                    &issue_id,
                    input_tokens,
                    output_tokens,
                    total_tokens,
                );
            }
            ServiceEvent::CodexRateLimits { payload } => {
                self.shared.runtime.lock().await.codex_rate_limits = Some(payload);
            }
            ServiceEvent::CodexActivity {
                issue_id,
                event,
                message,
                at,
            } => {
                {
                    let mut runtime = self.shared.runtime.lock().await;
                    if let Some(entry) = runtime.running.get_mut(&issue_id) {
                        entry.live_session.last_codex_event = Some(event.clone());
                        entry.live_session.last_codex_timestamp = Some(at);
                        entry.live_session.last_codex_message = message.clone();
                    }
                }

                let mut events = self.shared.recent_events.lock().await;
                let buffer = events.entry(issue_id).or_insert_with(VecDeque::new);
                buffer.push_front(RecentEvent { at, event, message });
                buffer.truncate(RECENT_EVENT_LIMIT);
            }
            ServiceEvent::RetryDue { issue_id } => {
                self.retry_handles.remove(&issue_id);
                self.handle_retry_due(issue_id).await;
            }
            ServiceEvent::ForceRefresh => {
                self.process_tick().await;
            }
        }
    }

    async fn handle_retry_due(&mut self, issue_id: IssueId) {
        let retry = {
            let mut runtime = self.shared.runtime.lock().await;
            runtime.retry_attempts.remove(&issue_id)
        };

        let Some(retry_entry) = retry else {
            return;
        };

        let loaded = self.shared.workflow.read().await.clone();
        let active_states = loaded
            .config
            .tracker
            .active_states
            .iter()
            .map(|state| state.value().to_string())
            .collect::<Vec<_>>();

        let candidates = match self.tracker.fetch_candidate_issues(&active_states).await {
            Ok(issues) => issues,
            Err(error) => {
                error!(reason = %error, "retry poll failed");
                self.reschedule_retry(
                    retry_entry.issue_id,
                    retry_entry.identifier,
                    retry_entry.attempt.saturating_add(1),
                    "retry poll failed".to_string(),
                )
                .await;
                return;
            }
        };

        let maybe_issue = candidates.into_iter().find(|issue| issue.id == issue_id);

        let Some(issue) = maybe_issue else {
            self.shared.runtime.lock().await.claimed.remove(&issue_id);
            return;
        };

        let policy = DispatchPolicy {
            active_states: parse_state_set(&loaded.config.tracker.active_states),
            terminal_states: parse_state_set(&loaded.config.tracker.terminal_states),
            agent: loaded.config.agent.clone(),
        };

        let retry_action = {
            let runtime = self.shared.runtime.lock().await;
            decide_retry_action(&issue, &runtime, &policy)
        };

        match retry_action {
            RetryAction::Dispatch => {
                self.dispatch_issue(issue, Some(retry_entry.attempt), true)
                    .await;
            }
            RetryAction::RequeueNoSlots => {
                self.reschedule_retry(
                    retry_entry.issue_id,
                    issue.identifier,
                    retry_entry.attempt.saturating_add(1),
                    "no available orchestrator slots".to_string(),
                )
                .await;
            }
            RetryAction::ReleaseClaim => {
                info!(
                    issue_id = %retry_entry.issue_id.value(),
                    issue_identifier = %retry_entry.identifier.value(),
                    "releasing retry claim because issue is not currently dispatch-eligible"
                );
                self.shared
                    .runtime
                    .lock()
                    .await
                    .claimed
                    .remove(&retry_entry.issue_id);
            }
        }
    }

    async fn reschedule_retry(
        &mut self,
        issue_id: IssueId,
        identifier: IssueIdentifier,
        attempt: u32,
        error_message: String,
    ) {
        let now_ms = epoch_millis();
        let max_backoff = self
            .shared
            .workflow
            .read()
            .await
            .config
            .agent
            .max_retry_backoff_ms
            .value();
        let plan = RetryPlan {
            issue_id: issue_id.clone(),
            attempt,
            due_after_ms: symphony_core::failure_backoff_ms(attempt, max_backoff),
            error: Some(error_message),
        };

        let retry_entry = RetryEntry {
            issue_id: issue_id.clone(),
            identifier,
            attempt: plan.attempt,
            due_at_ms: now_ms.saturating_add(plan.due_after_ms),
            error: plan.error,
        };

        {
            let mut runtime = self.shared.runtime.lock().await;
            runtime.retry_attempts.insert(issue_id, retry_entry.clone());
        }

        self.schedule_retry_timer(&retry_entry).await;
    }

    async fn schedule_retry_timer(&mut self, retry_entry: &RetryEntry) {
        if let Some(existing) = self.retry_handles.remove(&retry_entry.issue_id) {
            existing.abort();
        }

        let now_ms = epoch_millis();
        let delay_ms = retry_entry.due_at_ms.saturating_sub(now_ms);
        let issue_id = retry_entry.issue_id.clone();
        let event_tx = self.shared.event_tx.clone();

        let handle = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            let _ = event_tx.send(ServiceEvent::RetryDue { issue_id });
        });

        self.retry_handles
            .insert(retry_entry.issue_id.clone(), handle);
    }

    async fn reconcile_running_issues(&mut self) {
        self.reconcile_stalled_runs().await;

        let running_ids = {
            let runtime = self.shared.runtime.lock().await;
            runtime
                .running
                .keys()
                .map(|issue_id| issue_id.value().to_string())
                .collect::<Vec<_>>()
        };

        if running_ids.is_empty() {
            return;
        }

        let refreshed = match self.tracker.fetch_issue_states_by_ids(&running_ids).await {
            Ok(issues) => issues,
            Err(error) => {
                debug!(reason = %error, "reconciliation state refresh failed; keeping workers running");
                return;
            }
        };

        let loaded = self.shared.workflow.read().await.clone();
        let active_states = parse_state_set(&loaded.config.tracker.active_states);
        let terminal_states = parse_state_set(&loaded.config.tracker.terminal_states);

        let refreshed_by_id = refreshed
            .into_iter()
            .map(|issue| (issue.id.clone(), issue))
            .collect::<HashMap<_, _>>();

        let tracked_ids = {
            self.shared
                .runtime
                .lock()
                .await
                .running
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        };

        for issue_id in tracked_ids {
            let Some(refreshed_issue) = refreshed_by_id.get(&issue_id) else {
                self.stop_running_issue(&issue_id, false).await;
                continue;
            };

            let normalized_state =
                symphony_domain::normalize_state_name(refreshed_issue.state.value());
            if terminal_states.contains(&normalized_state) {
                self.stop_running_issue(&issue_id, true).await;
                continue;
            }

            if active_states.contains(&normalized_state) {
                if let Some(entry) = self.shared.runtime.lock().await.running.get_mut(&issue_id) {
                    entry.issue = refreshed_issue.clone();
                }
                continue;
            }

            self.stop_running_issue(&issue_id, false).await;
        }
    }

    async fn reconcile_stalled_runs(&mut self) {
        let loaded = self.shared.workflow.read().await.clone();
        let stall_timeout_ms = loaded.config.codex.stall_timeout_ms;

        if stall_timeout_ms <= 0 {
            return;
        }

        let now = Utc::now();
        let candidates = {
            let runtime = self.shared.runtime.lock().await;
            runtime
                .running
                .iter()
                .filter_map(|(issue_id, entry)| {
                    let reference = entry
                        .live_session
                        .last_codex_timestamp
                        .unwrap_or(entry.started_at);
                    let elapsed = now.signed_duration_since(reference).num_milliseconds();
                    if is_stalled(elapsed, stall_timeout_ms) {
                        Some(issue_id.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        };

        if candidates.is_empty() {
            return;
        }

        for issue_id in candidates {
            if let Some(handle) = self.worker_handles.remove(&issue_id) {
                handle.abort();
            }

            let retry = {
                let mut runtime = self.shared.runtime.lock().await;
                apply_worker_exit(
                    &mut runtime,
                    &issue_id,
                    WorkerExitReason::Stalled,
                    epoch_millis(),
                    loaded.config.agent.max_retry_backoff_ms.value(),
                )
            };

            if let Some(retry_entry) = retry {
                self.schedule_retry_timer(&retry_entry).await;
            }
        }
    }

    async fn stop_running_issue(&mut self, issue_id: &IssueId, cleanup_workspace: bool) {
        if let Some(handle) = self.worker_handles.remove(issue_id) {
            handle.abort();
        }

        let removed = {
            let mut runtime = self.shared.runtime.lock().await;
            runtime.retry_attempts.remove(issue_id);
            runtime.claimed.remove(issue_id);
            runtime.running.remove(issue_id)
        };

        if cleanup_workspace && let Some(entry) = removed {
            let config = self.shared.workflow.read().await.config.clone();
            match WorkspaceManager::new(&config) {
                Ok(manager) => {
                    if let Err(error) = manager
                        .remove_workspace(&entry.issue.identifier, &config)
                        .await
                    {
                        warn!(reason = %error, issue_identifier = %entry.issue.identifier.value(), "failed to remove workspace during reconciliation cleanup");
                    }
                }
                Err(error) => {
                    warn!(reason = %error, "failed to initialize workspace manager for cleanup");
                }
            }
        }
    }

    async fn shutdown_workers(&mut self) {
        for handle in self.worker_handles.drain().map(|(_, handle)| handle) {
            handle.abort();
        }
        for handle in self.retry_handles.drain().map(|(_, handle)| handle) {
            handle.abort();
        }

        let _ = &self.workflow_watcher;
    }
}

#[async_trait]
impl IssueTracker for LinearTracker {
    async fn fetch_candidate_issues(
        &self,
        active_states: &[String],
    ) -> Result<Vec<Issue>, AppError> {
        self.fetch_issues_by_state_names(active_states).await
    }

    async fn fetch_issues_by_states(&self, states: &[String]) -> Result<Vec<Issue>, AppError> {
        if states.is_empty() {
            return Ok(Vec::new());
        }
        self.fetch_issues_by_state_names(states).await
    }

    async fn fetch_issue_states_by_ids(
        &self,
        issue_ids: &[String],
    ) -> Result<Vec<Issue>, AppError> {
        if issue_ids.is_empty() {
            return Ok(Vec::new());
        }

        let query = r#"
query IssueStatesByIds($ids: [ID!]) {
  issues(filter: { id: { in: $ids } }) {
    nodes {
      id
      identifier
      title
      state { name }
      priority
      createdAt
      updatedAt
    }
  }
}
"#;

        let payload = self
            .graphql(
                query,
                serde_json::json!({
                    "ids": issue_ids,
                }),
            )
            .await?;

        parse_issue_nodes(&payload)
    }

    async fn execute_raw_graphql(
        &self,
        query: &str,
        variables: Option<serde_json::Map<String, Value>>,
    ) -> Result<Value, AppError> {
        let mut body = serde_json::Map::new();
        body.insert("query".to_string(), Value::String(query.to_string()));
        if let Some(variables) = variables {
            body.insert("variables".to_string(), Value::Object(variables));
        }

        let response = self
            .http_client
            .post(&self.endpoint)
            .header("Authorization", self.api_key.clone())
            .json(&Value::Object(body))
            .send()
            .await
            .map_err(|error| AppError::LinearApiRequest(error.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<unreadable>".to_string());
            return Err(AppError::LinearApiStatus(format!(
                "status={status} body={body}"
            )));
        }

        let payload = response
            .json::<Value>()
            .await
            .map_err(|error| AppError::LinearApiRequest(error.to_string()))?;

        if let Some(errors) = payload.get("errors") {
            return Err(AppError::LinearGraphqlErrors(errors.to_string()));
        }

        Ok(payload)
    }
}

impl LinearTracker {
    fn new(config: &TrackerConfig) -> Result<Self, AppError> {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_millis(30_000))
            .build()
            .map_err(|error| AppError::LinearApiRequest(error.to_string()))?;

        Ok(Self {
            endpoint: config.endpoint.as_ref().to_string(),
            api_key: config.api_key.value().to_string(),
            project_slug: config.project_slug.value().to_string(),
            http_client,
        })
    }

    async fn fetch_issues_by_state_names(&self, states: &[String]) -> Result<Vec<Issue>, AppError> {
        let query = r#"
query CandidateIssues($projectSlug: String!, $stateNames: [String!], $after: String, $first: Int!) {
  issues(
    first: $first,
    after: $after,
    filter: {
      project: { slugId: { eq: $projectSlug } }
      state: { name: { in: $stateNames } }
    }
  ) {
    pageInfo {
      hasNextPage
      endCursor
    }
    nodes {
      id
      identifier
      title
      description
      priority
      branchName
      url
      createdAt
      updatedAt
      state {
        name
      }
      labels {
        nodes {
          name
        }
      }
      inverseRelations {
        nodes {
          type
          relatedIssue {
            id
            identifier
            state {
              name
            }
          }
        }
      }
    }
  }
}
"#;

        let mut after: Option<String> = None;
        let mut issues = Vec::new();

        loop {
            let payload = self
                .graphql(
                    query,
                    serde_json::json!({
                        "projectSlug": self.project_slug,
                        "stateNames": states,
                        "after": after,
                        "first": 50,
                    }),
                )
                .await?;

            let mut parsed = parse_issue_nodes(&payload)?;
            issues.append(&mut parsed);

            let page_info = payload
                .pointer("/data/issues/pageInfo")
                .and_then(Value::as_object)
                .ok_or_else(|| AppError::LinearApiRequest("linear_unknown_payload".to_string()))?;

            let has_next_page = page_info
                .get("hasNextPage")
                .and_then(Value::as_bool)
                .unwrap_or(false);

            if !has_next_page {
                break;
            }

            after = page_info
                .get("endCursor")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);

            if after.is_none() {
                return Err(AppError::LinearApiRequest(
                    "linear_missing_end_cursor".to_string(),
                ));
            }
        }

        Ok(issues)
    }

    async fn graphql(&self, query: &str, variables: Value) -> Result<Value, AppError> {
        self.execute_raw_graphql(query, variables.as_object().cloned())
            .await
    }
}

impl WorkspaceManager {
    fn new(config: &ServiceConfig) -> Result<Self, AppError> {
        let root = PathBuf::from(config.workspace.root.value());
        let root = if root.is_absolute() {
            root
        } else {
            std::env::current_dir()
                .map_err(|error| AppError::WorkspaceError(error.to_string()))?
                .join(root)
        };

        std::fs::create_dir_all(&root)
            .map_err(|error| AppError::WorkspaceError(error.to_string()))?;

        Ok(Self { root })
    }

    async fn ensure_workspace(
        &self,
        issue_identifier: &IssueIdentifier,
        config: &ServiceConfig,
    ) -> Result<Workspace, AppError> {
        let workspace_key = sanitize_workspace_key(issue_identifier.value());
        let path = self.root.join(&workspace_key);
        ensure_workspace_inside_root(&self.root, &path)?;

        let created_now = if path.exists() {
            if !path.is_dir() {
                return Err(AppError::WorkspaceError(format!(
                    "workspace path exists and is not directory: {}",
                    path.display()
                )));
            }
            false
        } else {
            std::fs::create_dir_all(&path)
                .map_err(|error| AppError::WorkspaceError(error.to_string()))?;
            true
        };

        let workspace = Workspace { path, created_now };

        let initialization_marker = workspace.path.join(".symphony_workspace_initialized");
        let needs_workspace_initialization =
            workspace.created_now || !initialization_marker.exists();

        if needs_workspace_initialization {
            if let Some(script) = config.hooks.after_create.as_deref() {
                run_hook(
                    "after_create",
                    script,
                    &workspace.path,
                    config.hooks.timeout_ms.value(),
                    true,
                )
                .await?;
            }
            std::fs::write(&initialization_marker, b"initialized\n")
                .map_err(|error| AppError::WorkspaceError(error.to_string()))?;
        }

        Ok(workspace)
    }

    async fn run_before_run(
        &self,
        workspace: &Workspace,
        config: &ServiceConfig,
    ) -> Result<(), AppError> {
        if let Some(script) = config.hooks.before_run.as_deref() {
            run_hook(
                "before_run",
                script,
                &workspace.path,
                config.hooks.timeout_ms.value(),
                true,
            )
            .await?;
        }
        Ok(())
    }

    async fn run_after_run_best_effort(&self, workspace: &Workspace, config: &ServiceConfig) {
        if let Some(script) = config.hooks.after_run.as_deref()
            && let Err(error) = run_hook(
                "after_run",
                script,
                &workspace.path,
                config.hooks.timeout_ms.value(),
                false,
            )
            .await
        {
            warn!(reason = %error, workspace = %workspace.path.display(), "after_run hook failed");
        }
    }

    async fn remove_workspace(
        &self,
        issue_identifier: &IssueIdentifier,
        config: &ServiceConfig,
    ) -> Result<(), AppError> {
        let workspace_key = sanitize_workspace_key(issue_identifier.value());
        let path = self.root.join(workspace_key);
        if !path.exists() {
            return Ok(());
        }

        ensure_workspace_inside_root(&self.root, &path)?;

        if let Some(script) = config.hooks.before_remove.as_deref()
            && let Err(error) = run_hook(
                "before_remove",
                script,
                &path,
                config.hooks.timeout_ms.value(),
                false,
            )
            .await
        {
            warn!(reason = %error, workspace = %path.display(), "before_remove hook failed");
        }

        std::fs::remove_dir_all(&path)
            .map_err(|error| AppError::WorkspaceError(error.to_string()))?;
        Ok(())
    }
}

async fn run_agent_attempt(
    launch: WorkerLaunchConfig,
    tracker: Arc<dyn IssueTracker>,
    event_tx: UnboundedSender<ServiceEvent>,
) -> WorkerExitReason {
    let manager = match WorkspaceManager::new(&launch.config) {
        Ok(manager) => manager,
        Err(error) => return WorkerExitReason::Failed(error.to_string()),
    };

    let workspace = match manager
        .ensure_workspace(&launch.issue.identifier, &launch.config)
        .await
    {
        Ok(workspace) => workspace,
        Err(error) => return WorkerExitReason::Failed(error.to_string()),
    };

    let result = async {
        manager
            .run_before_run(&workspace, &launch.config)
            .await
            .map_err(|error| WorkerExitReason::Failed(error.to_string()))?;

        run_codex_turn_loop(&launch, tracker, &workspace, event_tx).await
    }
    .await;

    manager
        .run_after_run_best_effort(&workspace, &launch.config)
        .await;

    match result {
        Ok(()) => WorkerExitReason::Normal,
        Err(error) => error,
    }
}

async fn run_codex_turn_loop(
    launch: &WorkerLaunchConfig,
    tracker: Arc<dyn IssueTracker>,
    workspace: &Workspace,
    event_tx: UnboundedSender<ServiceEvent>,
) -> Result<(), WorkerExitReason> {
    let prompt_template = if launch.workflow.prompt_template.trim().is_empty() {
        DEFAULT_PROMPT.to_string()
    } else {
        launch.workflow.prompt_template.clone()
    };

    let first_prompt = render_issue_prompt(&prompt_template, &launch.issue, launch.attempt)
        .map_err(|error| WorkerExitReason::Failed(error.to_string()))?;

    let workspace_path = workspace
        .path
        .canonicalize()
        .map_err(|error| WorkerExitReason::Failed(error.to_string()))?;

    ensure_workspace_inside_root(
        &PathBuf::from(launch.config.workspace.root.value()),
        &workspace_path,
    )
    .map_err(|error| WorkerExitReason::Failed(error.to_string()))?;

    let mut child = Command::new("bash")
        .arg("-lc")
        .arg(launch.config.codex.command.value())
        .current_dir(&workspace_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| WorkerExitReason::Failed(format!("codex_not_found: {error}")))?;

    let pid = child.id().map(|value| value.to_string());

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| WorkerExitReason::Failed("missing stderr pipe".to_string()))?;
    tokio::spawn(log_stderr(stderr));

    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| WorkerExitReason::Failed("missing stdin pipe".to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| WorkerExitReason::Failed("missing stdout pipe".to_string()))?;

    let mut io = CodexIo {
        issue_id: &launch.issue.id,
        tracker,
        event_tx,
        stdin,
        stdout: BufReader::new(stdout),
        read_timeout_ms: launch.config.codex.read_timeout_ms.value(),
    };

    let mut next_id = 1_i64;
    io.send_json(&serde_json::json!({
        "id": next_id,
        "method": "initialize",
        "params": {
            "clientInfo": {
                "name": "symphony-rs",
                "version": env!("CARGO_PKG_VERSION")
            },
            "capabilities": {}
        }
    }))
    .await
    .map_err(|error| WorkerExitReason::Failed(error.to_string()))?;

    let _ = io
        .wait_for_response(next_id)
        .await
        .map_err(|error| WorkerExitReason::Failed(error.to_string()))?;

    io.send_json(&serde_json::json!({
        "method": "initialized",
        "params": {}
    }))
    .await
    .map_err(|error| WorkerExitReason::Failed(error.to_string()))?;

    next_id = next_id.saturating_add(1);
    io.send_json(&serde_json::json!({
        "id": next_id,
        "method": "thread/start",
        "params": {
            "approvalPolicy": launch.config.codex.approval_policy.value(),
            "sandbox": launch.config.codex.thread_sandbox.as_ref(),
            "cwd": workspace_path.to_string_lossy(),
        }
    }))
    .await
    .map_err(|error| WorkerExitReason::Failed(error.to_string()))?;

    let thread_response = io
        .wait_for_response(next_id)
        .await
        .map_err(|error| WorkerExitReason::Failed(error.to_string()))?;
    let thread_id = extract_string(&thread_response, &["result", "thread", "id"])
        .ok_or_else(|| WorkerExitReason::Failed("missing thread id".to_string()))?;

    let mut issue = launch.issue.clone();
    let max_turns = launch.config.agent.max_turns.value();
    let mut turn_number = 1_u32;

    loop {
        let prompt = if turn_number == 1 {
            first_prompt.clone()
        } else {
            CONTINUATION_PROMPT.to_string()
        };

        next_id = next_id.saturating_add(1);
        io.send_json(&serde_json::json!({
            "id": next_id,
            "method": "turn/start",
            "params": {
                "threadId": thread_id,
                "input": [{"type": "text", "text": prompt}],
                "cwd": workspace_path.to_string_lossy(),
                "title": format!("{}: {}", issue.identifier.value(), issue.title.value()),
                "approvalPolicy": launch.config.codex.approval_policy.value(),
                "sandboxPolicy": launch.config.codex.turn_sandbox_policy.json,
            }
        }))
        .await
        .map_err(|error| WorkerExitReason::Failed(error.to_string()))?;

        let turn_response = io
            .wait_for_response(next_id)
            .await
            .map_err(|error| WorkerExitReason::Failed(error.to_string()))?;
        let turn_id = extract_string(&turn_response, &["result", "turn", "id"])
            .ok_or_else(|| WorkerExitReason::Failed("missing turn id".to_string()))?;

        let session_id = format!("{thread_id}-{turn_id}");
        let _ = io.event_tx.send(ServiceEvent::CodexSessionStarted {
            issue_id: issue.id.clone(),
            session_id,
            thread_id: thread_id.clone(),
            turn_id: turn_id.clone(),
            pid: pid.clone(),
        });

        let turn_outcome = io
            .stream_turn(launch.config.codex.turn_timeout_ms.value())
            .await
            .map_err(|error| {
                if let Some(code) = protocol_retry_error(&error) {
                    WorkerExitReason::Failed(code.to_string())
                } else {
                    WorkerExitReason::Failed(error.to_string())
                }
            })?;

        match turn_outcome {
            TurnOutcome::Completed => {}
            TurnOutcome::Failed => return Err(WorkerExitReason::Failed("turn_failed".to_string())),
            TurnOutcome::Cancelled => {
                return Err(WorkerExitReason::Failed("turn_cancelled".to_string()));
            }
        }

        let refreshed = io
            .tracker
            .fetch_issue_states_by_ids(&[issue.id.value().to_string()])
            .await
            .map_err(|error| WorkerExitReason::Failed(error.to_string()))?;

        if let Some(next_issue) = refreshed.into_iter().next() {
            issue = next_issue;
        }

        let active_states = parse_state_set(&launch.config.tracker.active_states);
        if !active_states.contains(&symphony_domain::normalize_state_name(issue.state.value())) {
            break;
        }

        if turn_number >= max_turns {
            break;
        }

        turn_number = turn_number.saturating_add(1);
    }

    if let Err(error) = stop_codex_child(child).await {
        warn!(reason = %error, "failed to stop codex process cleanly");
    }

    Ok(())
}

impl<'a> CodexIo<'a> {
    async fn send_json(&mut self, payload: &Value) -> Result<(), AppError> {
        let rendered = serde_json::to_string(payload)
            .map_err(|error| AppError::ProtocolError(error.to_string()))?;
        self.stdin
            .write_all(rendered.as_bytes())
            .await
            .map_err(|error| AppError::ProtocolError(error.to_string()))?;
        self.stdin
            .write_all(b"\n")
            .await
            .map_err(|error| AppError::ProtocolError(error.to_string()))?;
        self.stdin
            .flush()
            .await
            .map_err(|error| AppError::ProtocolError(error.to_string()))
    }

    async fn wait_for_response(&mut self, expected_id: i64) -> Result<Value, AppError> {
        loop {
            match self.read_next(self.read_timeout_ms).await? {
                ReadResult::TimedOut => {
                    return Err(AppError::ProtocolError("response_timeout".to_string()));
                }
                ReadResult::Message(message) => {
                    if message
                        .get("id")
                        .and_then(Value::as_i64)
                        .is_some_and(|value| value == expected_id)
                    {
                        if let Some(error_value) = message.get("error") {
                            return Err(AppError::ProtocolError(format!(
                                "response_error: {error_value}"
                            )));
                        }
                        return Ok(message);
                    }

                    self.handle_message(message).await?;
                }
            }
        }
    }

    async fn stream_turn(&mut self, timeout_ms: u64) -> Result<TurnOutcome, AppError> {
        let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);

        loop {
            let now = std::time::Instant::now();
            if now >= deadline {
                return Err(AppError::ProtocolError("turn_timeout".to_string()));
            }

            let remaining_ms = deadline
                .saturating_duration_since(now)
                .as_millis()
                .min(self.read_timeout_ms as u128) as u64;
            match self.read_next(remaining_ms).await? {
                ReadResult::TimedOut => continue,
                ReadResult::Message(message) => {
                    let method = message
                        .get("method")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();

                    self.emit_activity(&message, &method).await;
                    self.capture_usage_and_limits(&message).await;

                    let lower_method = method.to_lowercase();
                    if lower_method.contains("turn/completed") {
                        return Ok(TurnOutcome::Completed);
                    }
                    if lower_method.contains("turn/failed") {
                        return Ok(TurnOutcome::Failed);
                    }
                    if lower_method.contains("turn/cancelled")
                        || lower_method.contains("turn/canceled")
                    {
                        return Ok(TurnOutcome::Cancelled);
                    }

                    if contains_user_input_required(&message) {
                        return Err(AppError::ProtocolError("turn_input_required".to_string()));
                    }

                    self.handle_message(message).await?;
                }
            }
        }
    }

    async fn read_next(&mut self, timeout_ms: u64) -> Result<ReadResult, AppError> {
        let mut line = String::new();
        let timeout = Duration::from_millis(timeout_ms.max(1));
        let read_result = tokio::time::timeout(timeout, self.stdout.read_line(&mut line)).await;

        let bytes_read = match read_result {
            Ok(Ok(value)) => value,
            Ok(Err(error)) => return Err(AppError::ProtocolError(error.to_string())),
            Err(_) => return Ok(ReadResult::TimedOut),
        };

        if bytes_read == 0 {
            return Err(AppError::ProtocolError("port_exit".to_string()));
        }

        Ok(ReadResult::Message(parse_protocol_message_line(&line)?))
    }

    async fn handle_message(&mut self, message: Value) -> Result<(), AppError> {
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_lowercase();

        if contains_user_input_required(&message) {
            return Err(AppError::ProtocolError("turn_input_required".to_string()));
        }

        let Some(request_id) = message.get("id").cloned() else {
            return Ok(());
        };

        if method.contains("approval") {
            self.send_json(&serde_json::json!({
                "id": request_id,
                "result": { "approved": true },
            }))
            .await?;
            let _ = self.event_tx.send(ServiceEvent::CodexActivity {
                issue_id: self.issue_id.clone(),
                event: "approval_auto_approved".to_string(),
                message: None,
                at: Utc::now(),
            });
            return Ok(());
        }

        if method.contains("tool/call") {
            self.handle_tool_call(&message).await?;
            return Ok(());
        }

        Ok(())
    }

    async fn handle_tool_call(&mut self, message: &Value) -> Result<(), AppError> {
        let request_id = message.get("id").cloned().unwrap_or(Value::Null);
        let name = extract_string(message, &["params", "name"]).unwrap_or_default();
        if name != "linear_graphql" {
            self.send_json(&serde_json::json!({
                "id": request_id,
                "result": { "success": false, "error": "unsupported_tool_call" },
            }))
            .await?;
            return Ok(());
        }

        let raw_input = message
            .pointer("/params/input")
            .cloned()
            .unwrap_or(Value::Null);

        let (query, variables) = if let Some(query) = raw_input.as_str() {
            (query.to_string(), None)
        } else {
            let query = raw_input
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let variables = raw_input
                .get("variables")
                .and_then(Value::as_object)
                .cloned();
            (query, variables)
        };

        if query.trim().is_empty() {
            self.send_json(&serde_json::json!({
                "id": request_id,
                "result": { "success": false, "error": "invalid_query" },
            }))
            .await?;
            return Ok(());
        }

        if graphql_operation_count(&query) != 1 {
            self.send_json(&serde_json::json!({
                "id": request_id,
                "result": { "success": false, "error": "expected_single_operation" },
            }))
            .await?;
            return Ok(());
        }

        let result = self.tracker.execute_raw_graphql(&query, variables).await;

        match result {
            Ok(payload) => {
                self.send_json(&serde_json::json!({
                    "id": request_id,
                    "result": {
                        "success": true,
                        "response": payload,
                    }
                }))
                .await?;
            }
            Err(error) => {
                self.send_json(&serde_json::json!({
                    "id": request_id,
                    "result": {
                        "success": false,
                        "error": error.to_string(),
                    }
                }))
                .await?;
            }
        }

        Ok(())
    }

    async fn emit_activity(&self, message: &Value, event: &str) {
        let summary = summarize_message(message);
        let _ = self.event_tx.send(ServiceEvent::CodexActivity {
            issue_id: self.issue_id.clone(),
            event: if event.is_empty() {
                "other_message".to_string()
            } else {
                event.to_string()
            },
            message: summary,
            at: Utc::now(),
        });
    }

    async fn capture_usage_and_limits(&self, message: &Value) {
        if let Some((input_tokens, output_tokens, total_tokens)) =
            extract_absolute_token_totals(message)
        {
            let _ = self.event_tx.send(ServiceEvent::CodexTokenTotals {
                issue_id: self.issue_id.clone(),
                input_tokens,
                output_tokens,
                total_tokens,
            });
        }

        if let Some(rate_limits) = extract_rate_limits(message) {
            let _ = self.event_tx.send(ServiceEvent::CodexRateLimits {
                payload: rate_limits,
            });
        }
    }
}

enum ReadResult {
    TimedOut,
    Message(Value),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetryAction {
    Dispatch,
    RequeueNoSlots,
    ReleaseClaim,
}

fn decide_retry_action(
    issue: &Issue,
    runtime: &symphony_domain::RuntimeState,
    policy: &DispatchPolicy,
) -> RetryAction {
    let mut projected_runtime = runtime.clone();
    projected_runtime.claimed.remove(&issue.id);

    if should_dispatch(issue, &projected_runtime, policy) {
        return RetryAction::Dispatch;
    }

    if retry_blocked_by_slots(issue, &projected_runtime, policy) {
        return RetryAction::RequeueNoSlots;
    }

    RetryAction::ReleaseClaim
}

fn retry_blocked_by_slots(
    issue: &Issue,
    runtime: &symphony_domain::RuntimeState,
    policy: &DispatchPolicy,
) -> bool {
    if available_slots(runtime, policy) == 0 {
        return true;
    }

    let normalized_state = symphony_domain::normalize_state_name(issue.state.value());
    let running_by_state = parse_running_state_count(runtime);
    let current_state_count = running_by_state
        .get(&normalized_state)
        .copied()
        .unwrap_or(0);

    let state_limit = policy
        .agent
        .max_concurrent_agents_by_state
        .get(&normalized_state)
        .copied()
        .unwrap_or(policy.agent.max_concurrent_agents)
        .value();

    current_state_count >= state_limit
}

fn dashboard_site_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join(site_root())
}

async fn run_http_server(shared: SharedAppState, port: u16) -> Result<(), AppError> {
    let site_root_path = dashboard_site_root();
    let site_pkg_path = site_root_path.join(site_pkg_dir());
    let package_route = format!("/{}", site_pkg_dir());

    let app = Router::new()
        .route("/", get(dashboard_handler))
        .route("/api/v1/state", get(api_state_handler))
        .route("/api/v1/refresh", post(api_refresh_handler))
        .route("/api/v1/{issue_identifier}", get(api_issue_handler))
        .nest_service(package_route.as_str(), ServeDir::new(site_pkg_path))
        .fallback_service(ServeDir::new(site_root_path))
        .layer(TraceLayer::new_for_http())
        .with_state(shared);

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|error| AppError::HttpError(error.to_string()))?;
    let local_addr = listener
        .local_addr()
        .map_err(|error| AppError::HttpError(error.to_string()))?;
    info!(addr = %local_addr, "http server listening");

    axum::serve(listener, app)
        .await
        .map_err(|error| AppError::HttpError(error.to_string()))
}

async fn dashboard_handler(
    State(shared): State<SharedAppState>,
    request: AxumRequest,
) -> Response {
    let runtime = shared.runtime.lock().await.clone();
    let snapshot = build_runtime_snapshot(&runtime, Utc::now());
    let handler = leptos_axum::render_app_async_with_context(
        || {},
        move || render_dashboard(snapshot.clone()),
    );

    handler(request).await
}

async fn api_state_handler(State(shared): State<SharedAppState>) -> Response {
    let runtime = shared.runtime.lock().await.clone();
    let snapshot = build_runtime_snapshot(&runtime, Utc::now());
    Json(snapshot).into_response()
}

async fn api_refresh_handler(State(shared): State<SharedAppState>) -> Response {
    let _ = shared.event_tx.send(ServiceEvent::ForceRefresh);
    (
        StatusCode::ACCEPTED,
        Json(RefreshResponse {
            queued: true,
            coalesced: false,
            requested_at: Utc::now(),
            operations: vec!["poll", "reconcile"],
        }),
    )
        .into_response()
}

async fn api_issue_handler(
    State(shared): State<SharedAppState>,
    AxumPath(issue_identifier): AxumPath<String>,
) -> Response {
    let runtime = shared.runtime.lock().await.clone();

    let running = runtime
        .running
        .values()
        .find(|entry| entry.issue.identifier.value() == issue_identifier);

    let retry = runtime
        .retry_attempts
        .values()
        .find(|entry| entry.identifier.value() == issue_identifier);

    let Some(issue_id) = running
        .map(|entry| entry.issue.id.clone())
        .or_else(|| retry.map(|entry| entry.issue_id.clone()))
    else {
        return (
            StatusCode::NOT_FOUND,
            Json(ApiErrorEnvelope {
                error: ApiErrorBody {
                    code: "issue_not_found",
                    message: format!("issue not found: {issue_identifier}"),
                },
            }),
        )
            .into_response();
    };

    let workflow = shared.workflow.read().await.clone();
    let workspace_path = PathBuf::from(workflow.config.workspace.root.value())
        .join(sanitize_workspace_key(&issue_identifier));

    let recent_events = shared
        .recent_events
        .lock()
        .await
        .get(&issue_id)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|event| IssueEvent {
            at: event.at,
            event: event.event,
            message: event.message,
        })
        .collect::<Vec<_>>();

    let response = IssueDebugResponse {
        issue_identifier: issue_identifier.clone(),
        issue_id: issue_id.value().to_string(),
        status: if running.is_some() {
            "running".to_string()
        } else {
            "retrying".to_string()
        },
        workspace: IssueWorkspace {
            path: workspace_path.to_string_lossy().to_string(),
        },
        attempts: IssueAttempts {
            restart_count: running
                .and_then(|entry| entry.retry_attempt)
                .or_else(|| retry.map(|entry| entry.attempt))
                .unwrap_or(0),
            current_retry_attempt: retry.map(|entry| entry.attempt).unwrap_or(0),
        },
        running: running.map(|entry| IssueRunning {
            session_id: entry
                .live_session
                .session_id
                .as_ref()
                .map(|value| value.value().to_string()),
            turn_count: entry.live_session.turn_count.value(),
            state: entry.issue.state.value().to_string(),
            started_at: entry.started_at,
            last_event: entry.live_session.last_codex_event.clone(),
            last_message: entry.live_session.last_codex_message.clone(),
            last_event_at: entry.live_session.last_codex_timestamp,
            tokens: IssueTokens {
                input_tokens: entry.live_session.codex_input_tokens.value(),
                output_tokens: entry.live_session.codex_output_tokens.value(),
                total_tokens: entry.live_session.codex_total_tokens.value(),
            },
        }),
        retry: retry.map(|entry| IssueRetry {
            attempt: entry.attempt,
            due_at: DateTime::from_timestamp_millis(entry.due_at_ms as i64)
                .unwrap_or_else(Utc::now),
            error: entry.error.clone(),
        }),
        recent_events,
        last_error: retry.and_then(|entry| entry.error.clone()),
    };

    Json(response).into_response()
}

async fn startup_terminal_workspace_cleanup(
    tracker: &dyn IssueTracker,
    manager: &WorkspaceManager,
    config: &ServiceConfig,
) -> Result<(), AppError> {
    let terminal_states = config
        .tracker
        .terminal_states
        .iter()
        .map(|state| state.value().to_string())
        .collect::<Vec<_>>();

    let issues = tracker.fetch_issues_by_states(&terminal_states).await?;
    for issue in issues {
        if let Err(error) = manager.remove_workspace(&issue.identifier, config).await {
            warn!(reason = %error, issue_identifier = %issue.identifier.value(), "failed to remove terminal workspace");
        }
    }
    Ok(())
}

fn discover_workflow_path(explicit_path: Option<PathBuf>) -> Result<PathBuf, AppError> {
    let path = explicit_path.unwrap_or_else(|| PathBuf::from("WORKFLOW.md"));
    if !path.exists() {
        return Err(AppError::MissingWorkflowFile);
    }
    Ok(path)
}

fn load_workflow_file(path: &Path) -> Result<LoadedWorkflow, AppError> {
    let contents = std::fs::read_to_string(path)
        .map_err(|error| AppError::WorkflowParseError(error.to_string()))?;
    let workflow = parse_workflow(&contents)
        .map_err(|error| AppError::WorkflowParseError(error.to_string()))?;
    let environment = std::env::vars().collect::<HashMap<_, _>>();
    let config = build_service_config(&workflow, &environment)
        .map_err(|error| AppError::ConfigError(error.to_string()))?;

    Ok(LoadedWorkflow {
        workflow_path: path.to_path_buf(),
        workflow,
        config,
    })
}

fn parse_protocol_message_line(line: &str) -> Result<Value, AppError> {
    if line.len() > MAX_PROTOCOL_LINE_BYTES {
        return Err(AppError::ProtocolError(
            "protocol line too large".to_string(),
        ));
    }

    let trimmed = line.trim();
    serde_json::from_str::<Value>(trimmed)
        .map_err(|error| AppError::ProtocolError(format!("malformed json line: {error}")))
}

fn workflow_reload_event_matches_path(event: &notify::Event, watched_path: &Path) -> bool {
    event.paths.iter().any(|path| {
        path == watched_path || path.canonicalize().is_ok_and(|canonical| canonical == watched_path)
    })
}

fn within_workflow_reload_debounce(previous_reload_at: Option<Instant>, now: Instant) -> bool {
    previous_reload_at.is_some_and(|previous_reload_at| {
        now.duration_since(previous_reload_at) < Duration::from_millis(50)
    })
}

fn workflow_reload_event_should_trigger(event: &notify::Event, watched_path: &Path) -> bool {
    let kind_triggers_reload = matches!(
        event.kind,
        notify::EventKind::Any
            | notify::EventKind::Create(_)
            | notify::EventKind::Modify(_)
            | notify::EventKind::Remove(_)
    );

    kind_triggers_reload && workflow_reload_event_matches_path(event, watched_path)
}

fn spawn_workflow_watcher(
    workflow_path: PathBuf,
    sender: UnboundedSender<ServiceEvent>,
) -> Result<RecommendedWatcher, AppError> {
    let watched_path = workflow_path
        .canonicalize()
        .unwrap_or(workflow_path.clone());
    let callback_path = watched_path.clone();
    let last_reload_at = Arc::new(StdMutex::new(None::<Instant>));
    let callback_last_reload_at = Arc::clone(&last_reload_at);

    let mut watcher = RecommendedWatcher::new(
        move |event: notify::Result<notify::Event>| {
            let Ok(event) = event else {
                return;
            };

            if !workflow_reload_event_should_trigger(&event, &callback_path) {
                return;
            }

            let now = Instant::now();
            let Ok(last_reload_at) = callback_last_reload_at.lock() else {
                return;
            };
            if within_workflow_reload_debounce(*last_reload_at, now) {
                return;
            }
            drop(last_reload_at);

            match load_workflow_file(&callback_path) {
                Ok(loaded) => {
                    if let Ok(mut last_reload_at) = callback_last_reload_at.lock() {
                        *last_reload_at = Some(now);
                    }
                    let _ = sender.send(ServiceEvent::WorkflowReloaded {
                        loaded: Box::new(loaded),
                    });
                }
                Err(error) => {
                    error!(reason = %error, "failed to reload workflow after file change");
                }
            }
        },
        Config::default(),
    )
    .map_err(|error| AppError::WorkflowParseError(error.to_string()))?;

    watcher
        .watch(&watched_path, RecursiveMode::NonRecursive)
        .map_err(|error| AppError::WorkflowParseError(error.to_string()))?;

    Ok(watcher)
}

async fn run_hook(
    name: &str,
    script: &str,
    cwd: &Path,
    timeout_ms: u64,
    fatal: bool,
) -> Result<(), AppError> {
    let mut child = Command::new("bash")
        .arg("-lc")
        .arg(script)
        .current_dir(cwd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| AppError::WorkspaceError(format!("{name} spawn failed: {error}")))?;

    let timeout = Duration::from_millis(timeout_ms.max(1));
    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(error)) => {
            return Err(AppError::WorkspaceError(format!("{name} failed: {error}")));
        }
        Err(_) => {
            if let Err(error) = child.kill().await {
                warn!(reason = %error, hook = name, "failed to kill timed out hook");
            }
            if fatal {
                return Err(AppError::WorkspaceError(format!("{name} hook timeout")));
            }
            warn!(hook = name, "hook timed out but is best-effort");
            return Ok(());
        }
    };

    if !status.success() {
        let detail = format!("{name} hook failed status={status}");
        if fatal {
            return Err(AppError::WorkspaceError(detail));
        }
        warn!(reason = %detail, "best-effort hook failed");
    }

    Ok(())
}

async fn stop_codex_child(mut child: Child) -> Result<(), AppError> {
    if child.id().is_none() {
        return Ok(());
    }

    match child.try_wait() {
        Ok(Some(_)) => return Ok(()),
        Ok(None) => {}
        Err(error) => return Err(AppError::CodexError(error.to_string())),
    }

    if let Err(error) = child.kill().await {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) => return Err(AppError::CodexError(error.to_string())),
            Err(wait_error) => return Err(AppError::CodexError(wait_error.to_string())),
        }
    }

    child
        .wait()
        .await
        .map_err(|error| AppError::CodexError(error.to_string()))?;
    Ok(())
}

async fn log_stderr(stderr: tokio::process::ChildStderr) {
    let mut reader = BufReader::new(stderr);
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {
                let text = line.trim();
                debug!(stream = "stderr", message = text, "codex-app-server");
            }
            Err(error) => {
                debug!(reason = %error, "failed reading codex stderr");
                break;
            }
        }
    }
}

fn parse_issue_nodes(payload: &Value) -> Result<Vec<Issue>, AppError> {
    let nodes = payload
        .pointer("/data/issues/nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::LinearApiRequest("linear_unknown_payload".to_string()))?;

    nodes.iter().map(parse_issue).collect()
}

fn parse_issue(node: &Value) -> Result<Issue, AppError> {
    let id = node
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::LinearApiRequest("missing issue.id".to_string()))?;
    let identifier = node
        .get("identifier")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::LinearApiRequest("missing issue.identifier".to_string()))?;
    let title = node
        .get("title")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::LinearApiRequest("missing issue.title".to_string()))?;
    let state_name = node
        .pointer("/state/name")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::LinearApiRequest("missing issue.state".to_string()))?;

    let labels = node
        .pointer("/labels/nodes")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.get("name").and_then(Value::as_str))
                .filter_map(|name| parse_label(name).ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let blocked_by = node
        .pointer("/inverseRelations/nodes")
        .and_then(Value::as_array)
        .map(|relations| {
            relations
                .iter()
                .filter(|relation| {
                    relation
                        .get("type")
                        .and_then(Value::as_str)
                        .is_some_and(|value| value.eq_ignore_ascii_case("blocks"))
                })
                .map(|relation| {
                    let blocker = relation.get("relatedIssue").cloned().unwrap_or(Value::Null);
                    BlockerRef {
                        id: blocker
                            .get("id")
                            .and_then(Value::as_str)
                            .and_then(|raw| parse_issue_id(raw).ok()),
                        identifier: blocker
                            .get("identifier")
                            .and_then(Value::as_str)
                            .and_then(|raw| parse_issue_identifier(raw).ok()),
                        state: blocker
                            .pointer("/state/name")
                            .and_then(Value::as_str)
                            .and_then(|raw| parse_issue_state(raw).ok()),
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(Issue {
        id: parse_issue_id(id).map_err(|error| AppError::LinearApiRequest(error.to_string()))?,
        identifier: parse_issue_identifier(identifier)
            .map_err(|error| AppError::LinearApiRequest(error.to_string()))?,
        title: parse_issue_title(title)
            .map_err(|error| AppError::LinearApiRequest(error.to_string()))?,
        description: node
            .get("description")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        priority: node
            .get("priority")
            .and_then(Value::as_i64)
            .and_then(|value| i32::try_from(value).ok()),
        state: parse_issue_state(state_name)
            .map_err(|error| AppError::LinearApiRequest(error.to_string()))?,
        branch_name: node
            .get("branchName")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        url: node
            .get("url")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        labels,
        blocked_by,
        created_at: node
            .get("createdAt")
            .and_then(Value::as_str)
            .and_then(parse_timestamp),
        updated_at: node
            .get("updatedAt")
            .and_then(Value::as_str)
            .and_then(parse_timestamp),
    })
}

fn parse_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .map(|value| value.with_timezone(&Utc))
        .ok()
}

fn extract_string(message: &Value, path: &[&str]) -> Option<String> {
    let mut current = message;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_str().map(ToOwned::to_owned)
}

fn protocol_retry_error(error: &AppError) -> Option<&str> {
    let AppError::ProtocolError(value) = error else {
        return None;
    };

    match value.as_str() {
        "turn_timeout" | "turn_input_required" => Some(value.as_str()),
        _ => None,
    }
}

fn contains_user_input_required(message: &Value) -> bool {
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_lowercase();

    if method.contains("requestuserinput") || method.contains("inputrequired") {
        return true;
    }

    message
        .pointer("/params/requiresUserInput")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn summarize_message(message: &Value) -> Option<String> {
    extract_string(message, &["params", "message"])
        .or_else(|| extract_string(message, &["message"]))
}

fn extract_absolute_token_totals(message: &Value) -> Option<(u64, u64, u64)> {
    let candidates = [
        message.pointer("/params/total_token_usage"),
        message.pointer("/params/token_usage/total_token_usage"),
        message.pointer("/params/usage"),
        message.pointer("/result/total_token_usage"),
    ];

    for candidate in candidates.into_iter().flatten() {
        if let (Some(input_tokens), Some(output_tokens), Some(total_tokens)) = (
            candidate.get("input_tokens").and_then(Value::as_u64),
            candidate.get("output_tokens").and_then(Value::as_u64),
            candidate.get("total_tokens").and_then(Value::as_u64),
        ) {
            return Some((input_tokens, output_tokens, total_tokens));
        }
    }

    None
}

fn extract_rate_limits(message: &Value) -> Option<Value> {
    message
        .pointer("/params/rate_limits")
        .cloned()
        .or_else(|| message.pointer("/params/rateLimits").cloned())
        .or_else(|| message.pointer("/rate_limits").cloned())
}

fn graphql_operation_count(query: &str) -> usize {
    query
        .split(|character: char| {
            character.is_whitespace() || matches!(character, '{' | '}' | '(' | ')' | ',')
        })
        .filter(|token| matches!(*token, "query" | "mutation" | "subscription"))
        .count()
}

fn increment_turn_count(turn_count: TurnCount) -> TurnCount {
    TurnCount::try_new(turn_count.value().saturating_add(1)).unwrap_or(turn_count)
}

fn ensure_workspace_inside_root(
    workspace_root: &Path,
    workspace_path: &Path,
) -> Result<(), AppError> {
    let root = workspace_root
        .canonicalize()
        .or_else(|_| {
            std::fs::create_dir_all(workspace_root)?;
            workspace_root.canonicalize()
        })
        .map_err(|error| AppError::WorkspaceError(error.to_string()))?;

    if !workspace_path.exists() {
        std::fs::create_dir_all(workspace_path)
            .map_err(|error| AppError::WorkspaceError(error.to_string()))?;
    }

    let workspace = workspace_path
        .canonicalize()
        .map_err(|error| AppError::WorkspaceError(error.to_string()))?;

    if !workspace.starts_with(&root) {
        return Err(AppError::WorkspaceError(format!(
            "invalid_workspace_cwd root={} workspace={}",
            root.display(),
            workspace.display()
        )));
    }

    Ok(())
}

fn epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn is_stalled(elapsed_ms: i64, stall_timeout_ms: i64) -> bool {
    elapsed_ms > stall_timeout_ms
}

fn configure_logging() {
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(true)
        .with_thread_names(true)
        .json()
        .finish();

    let _ = tracing::subscriber::set_global_default(subscriber);
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use chrono::Utc;
    use serde_json::Value;

    use symphony_domain::{
        BlockerRef, Issue, PositiveCount, PositiveMs, parse_issue_id, parse_issue_identifier,
        parse_issue_state, parse_issue_title,
    };

    use super::{
        AppError, DispatchPolicy, MAX_PROTOCOL_LINE_BYTES, RetryAction, decide_retry_action,
        graphql_operation_count, initial_runtime_state, is_stalled, parse_protocol_message_line,
        parse_state_set, register_running_issue, within_workflow_reload_debounce,
        workflow_reload_event_matches_path, workflow_reload_event_should_trigger,
    };

    fn make_issue(identifier: &str, state: &str) -> Issue {
        Issue {
            id: parse_issue_id(&format!("id-{identifier}")).expect("issue id should parse"),
            identifier: parse_issue_identifier(identifier).expect("issue identifier should parse"),
            title: parse_issue_title("Issue title").expect("issue title should parse"),
            description: None,
            priority: Some(1),
            state: parse_issue_state(state).expect("issue state should parse"),
            branch_name: None,
            url: None,
            labels: Vec::new(),
            blocked_by: Vec::new(),
            created_at: None,
            updated_at: None,
        }
    }

    fn make_policy(max_concurrent_agents: u32) -> DispatchPolicy {
        DispatchPolicy {
            active_states: parse_state_set(&[
                parse_issue_state("Todo").expect("state should parse"),
                parse_issue_state("In Progress").expect("state should parse"),
            ]),
            terminal_states: parse_state_set(&[
                parse_issue_state("Done").expect("state should parse"),
                parse_issue_state("Closed").expect("state should parse"),
            ]),
            agent: symphony_domain::AgentConfig {
                max_concurrent_agents: PositiveCount::try_new(max_concurrent_agents)
                    .expect("positive count should parse"),
                max_turns: PositiveCount::try_new(20).expect("positive count should parse"),
                max_retry_backoff_ms: PositiveMs::try_new(300_000)
                    .expect("positive ms should parse"),
                max_concurrent_agents_by_state: HashMap::new(),
            },
        }
    }

    #[test]
    fn graphql_operation_counter_requires_single_operation() {
        assert_eq!(graphql_operation_count("query X { viewer { id } }"), 1);
        assert_eq!(
            graphql_operation_count(
                "query X { viewer { id } } mutation Y { issueCreate(input: {}) { success } }"
            ),
            2,
        );
    }

    #[test]
    fn retry_action_dispatches_when_issue_is_only_claimed_for_retry() {
        let issue = make_issue("ABC-10", "In Progress");
        let mut runtime = initial_runtime_state(30_000, 10);
        runtime.claimed.insert(issue.id.clone());
        let policy = make_policy(10);

        let action = decide_retry_action(&issue, &runtime, &policy);
        assert_eq!(action, RetryAction::Dispatch);
    }

    #[test]
    fn retry_action_requeues_when_no_global_slots_are_available() {
        let running_issue = make_issue("ABC-11", "In Progress");
        let retry_issue = make_issue("ABC-12", "In Progress");
        let mut runtime = initial_runtime_state(30_000, 1);
        register_running_issue(&mut runtime, running_issue, None, Utc::now());
        runtime.claimed.insert(retry_issue.id.clone());
        let policy = make_policy(1);

        let action = decide_retry_action(&retry_issue, &runtime, &policy);
        assert_eq!(action, RetryAction::RequeueNoSlots);
    }

    #[test]
    fn retry_action_releases_claim_when_todo_issue_has_non_terminal_blocker() {
        let mut issue = make_issue("ABC-13", "Todo");
        issue.blocked_by.push(BlockerRef {
            id: None,
            identifier: None,
            state: Some(parse_issue_state("In Progress").expect("state should parse")),
        });

        let mut runtime = initial_runtime_state(30_000, 10);
        runtime.claimed.insert(issue.id.clone());
        runtime.completed = HashSet::new();
        let policy = make_policy(10);

        let action = decide_retry_action(&issue, &runtime, &policy);
        assert_eq!(action, RetryAction::ReleaseClaim);
    }

    #[test]
    fn protocol_line_parser_accepts_exact_limit_and_rejects_larger_lines() {
        let empty_line = "{\"padding\":\"\"}\n";
        let exact_fill_len = MAX_PROTOCOL_LINE_BYTES - empty_line.len();
        let exact_padding = "x".repeat(exact_fill_len);
        let exact_line = format!("{{\"padding\":\"{}\"}}\n", exact_padding);
        assert_eq!(exact_line.len(), MAX_PROTOCOL_LINE_BYTES);

        let parsed = parse_protocol_message_line(&exact_line).expect("exact limit should parse");
        assert_eq!(parsed.get("padding").and_then(Value::as_str), Some(exact_padding.as_str()));

        let oversized_line = format!(
            "{{\"padding\":\"{}\"}}\n",
            "x".repeat(exact_fill_len + 1)
        );
        assert_eq!(oversized_line.len(), MAX_PROTOCOL_LINE_BYTES + 1);

        let error = parse_protocol_message_line(&oversized_line)
            .expect_err("oversized protocol line should fail");
        assert!(matches!(
            error,
            AppError::ProtocolError(message) if message == "protocol line too large"
        ));
    }

    #[test]
    fn workflow_reload_event_requires_matching_path() {
        let event = notify::Event {
            kind: notify::EventKind::Modify(notify::event::ModifyKind::Any),
            paths: vec![PathBuf::from("/tmp/OTHER.md")],
            attrs: Default::default(),
        };

        assert!(!workflow_reload_event_matches_path(
            &event,
            Path::new("/tmp/WORKFLOW.md"),
        ));
        assert!(!workflow_reload_event_should_trigger(
            &event,
            Path::new("/tmp/WORKFLOW.md"),
        ));
    }

    #[test]
    fn workflow_reload_event_matches_canonical_equivalent_path() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let temp_root = std::env::temp_dir().join(format!(
            "symphony-reload-path-test-{}-{}",
            std::process::id(),
            unique
        ));
        let nested = temp_root.join("nested");
        fs::create_dir_all(&nested).expect("temp directories should be created");
        let workflow_path = temp_root.join("WORKFLOW.md");
        fs::write(&workflow_path, "tracker:\n  project_slug: demo\n")
            .expect("workflow file should be written");

        let watched_path = workflow_path
            .canonicalize()
            .expect("workflow path should canonicalize");
        let event = notify::Event {
            kind: notify::EventKind::Modify(notify::event::ModifyKind::Any),
            paths: vec![nested.join("..").join("WORKFLOW.md")],
            attrs: Default::default(),
        };

        assert!(workflow_reload_event_matches_path(&event, &watched_path));

        fs::remove_file(&workflow_path).expect("workflow file should be removed");
        fs::remove_dir_all(&temp_root).expect("temp directories should be removed");
    }

    #[test]
    fn workflow_reload_event_ignores_access_notifications() {
        let event = notify::Event {
            kind: notify::EventKind::Access(notify::event::AccessKind::Any),
            paths: vec![PathBuf::from("/tmp/WORKFLOW.md")],
            attrs: Default::default(),
        };

        assert!(workflow_reload_event_matches_path(
            &event,
            Path::new("/tmp/WORKFLOW.md"),
        ));
        assert!(!workflow_reload_event_should_trigger(
            &event,
            Path::new("/tmp/WORKFLOW.md"),
        ));
    }

    #[test]
    fn workflow_reload_debounce_only_blocks_strictly_earlier_than_boundary() {
        let now = Instant::now();
        let inside_window = now
            .checked_sub(Duration::from_millis(49))
            .expect("inside-window instant should exist");
        let boundary = now
            .checked_sub(Duration::from_millis(50))
            .expect("boundary instant should exist");
        let outside_window = now
            .checked_sub(Duration::from_millis(51))
            .expect("outside-window instant should exist");

        assert!(within_workflow_reload_debounce(Some(inside_window), now));
        assert!(!within_workflow_reload_debounce(Some(boundary), now));
        assert!(!within_workflow_reload_debounce(Some(outside_window), now));
    }

    #[test]
    fn workflow_reload_event_triggers_for_modify_on_watched_file() {
        let event = notify::Event {
            kind: notify::EventKind::Modify(notify::event::ModifyKind::Any),
            paths: vec![PathBuf::from("/tmp/WORKFLOW.md")],
            attrs: Default::default(),
        };

        assert!(workflow_reload_event_should_trigger(
            &event,
            Path::new("/tmp/WORKFLOW.md"),
        ));
    }

    #[test]
    fn stall_detection_requires_elapsed_to_exceed_timeout() {
        assert!(is_stalled(1_001, 1_000));
        assert!(!is_stalled(1_000, 1_000));
        assert!(!is_stalled(999, 1_000));
    }
}
