use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};

use symphony_domain::{
    AgentConfig, CodexTotals, Issue, IssueId, IssueStateName, NormalizedState, RetryEntry,
    RunningEntry, RuntimeState, normalize_state_name,
};

#[derive(Clone, Debug)]
pub struct DispatchPolicy {
    pub active_states: HashSet<String>,
    pub terminal_states: HashSet<String>,
    pub agent: AgentConfig,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkerExitReason {
    Normal,
    Failed(String),
    TimedOut,
    Stalled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetryPlan {
    pub issue_id: IssueId,
    pub attempt: u32,
    pub due_after_ms: u64,
    pub error: Option<String>,
}

pub fn initial_runtime_state(poll_interval_ms: u64, max_concurrent_agents: u32) -> RuntimeState {
    RuntimeState {
        poll_interval_ms,
        max_concurrent_agents,
        running: HashMap::new(),
        claimed: HashSet::new(),
        retry_attempts: HashMap::new(),
        completed: HashSet::new(),
        codex_totals: CodexTotals::default(),
        codex_rate_limits: None,
    }
}

pub fn available_slots(runtime: &RuntimeState, policy: &DispatchPolicy) -> u32 {
    let running_count = runtime.running.len() as u32;
    policy
        .agent
        .max_concurrent_agents
        .into_inner()
        .saturating_sub(running_count)
}

pub fn is_terminal_state(state: &IssueStateName, policy: &DispatchPolicy) -> bool {
    policy
        .terminal_states
        .contains(&normalize_state_name(state.value()))
}

pub fn is_active_state(state: &IssueStateName, policy: &DispatchPolicy) -> bool {
    policy
        .active_states
        .contains(&normalize_state_name(state.value()))
}

pub fn sort_for_dispatch(issues: &[Issue]) -> Vec<Issue> {
    let mut sorted = issues.to_vec();
    sorted.sort_by(compare_issue_priority);
    sorted
}

pub fn should_dispatch(issue: &Issue, runtime: &RuntimeState, policy: &DispatchPolicy) -> bool {
    if runtime.running.contains_key(&issue.id) || runtime.claimed.contains(&issue.id) {
        return false;
    }

    if !is_active_state(&issue.state, policy) || is_terminal_state(&issue.state, policy) {
        return false;
    }

    if available_slots(runtime, policy) == 0 {
        return false;
    }

    if !within_state_concurrency_limit(issue, runtime, policy) {
        return false;
    }

    if normalize_state_name(issue.state.value()) == "todo"
        && has_non_terminal_blocker(issue, policy)
    {
        return false;
    }

    true
}

pub fn register_running_issue(
    runtime: &mut RuntimeState,
    issue: Issue,
    retry_attempt: Option<u32>,
    started_at: DateTime<Utc>,
) {
    runtime.claimed.insert(issue.id.clone());
    runtime.retry_attempts.remove(&issue.id);
    runtime.running.insert(
        issue.id.clone(),
        RunningEntry {
            issue,
            retry_attempt,
            started_at,
            live_session: symphony_domain::LiveSession::default(),
        },
    );
}

pub fn compute_retry_plan(
    issue_id: IssueId,
    retry_attempt: Option<u32>,
    reason: WorkerExitReason,
    max_backoff_ms: u64,
) -> RetryPlan {
    match reason {
        WorkerExitReason::Normal => RetryPlan {
            issue_id,
            attempt: 1,
            due_after_ms: continuation_delay_ms(),
            error: None,
        },
        WorkerExitReason::Failed(error) => {
            let attempt = retry_attempt.unwrap_or(0) + 1;
            RetryPlan {
                issue_id,
                attempt,
                due_after_ms: failure_backoff_ms(attempt, max_backoff_ms),
                error: Some(error),
            }
        }
        WorkerExitReason::TimedOut => {
            let attempt = retry_attempt.unwrap_or(0) + 1;
            RetryPlan {
                issue_id,
                attempt,
                due_after_ms: failure_backoff_ms(attempt, max_backoff_ms),
                error: Some("turn_timeout".to_string()),
            }
        }
        WorkerExitReason::Stalled => {
            let attempt = retry_attempt.unwrap_or(0) + 1;
            RetryPlan {
                issue_id,
                attempt,
                due_after_ms: failure_backoff_ms(attempt, max_backoff_ms),
                error: Some("stalled".to_string()),
            }
        }
    }
}

pub fn apply_worker_exit(
    runtime: &mut RuntimeState,
    issue_id: &IssueId,
    reason: WorkerExitReason,
    due_at_ms: u64,
    max_backoff_ms: u64,
) -> Option<RetryEntry> {
    let running_entry = runtime.running.remove(issue_id)?;

    let plan = compute_retry_plan(
        issue_id.clone(),
        running_entry.retry_attempt,
        reason.clone(),
        max_backoff_ms,
    );

    if reason == WorkerExitReason::Normal {
        runtime.completed.insert(issue_id.clone());
    }

    let retry_entry = RetryEntry {
        issue_id: issue_id.clone(),
        identifier: running_entry.issue.identifier,
        attempt: plan.attempt,
        due_at_ms: due_at_ms + plan.due_after_ms,
        error: plan.error,
    };

    runtime
        .retry_attempts
        .insert(issue_id.clone(), retry_entry.clone());
    Some(retry_entry)
}

pub fn apply_absolute_token_totals(
    runtime: &mut RuntimeState,
    issue_id: &IssueId,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
) {
    let Some(running_entry) = runtime.running.get_mut(issue_id) else {
        return;
    };

    let delta_input = input_tokens.saturating_sub(
        running_entry
            .live_session
            .last_reported_input_tokens
            .into_inner(),
    );
    let delta_output = output_tokens.saturating_sub(
        running_entry
            .live_session
            .last_reported_output_tokens
            .into_inner(),
    );
    let delta_total = total_tokens.saturating_sub(
        running_entry
            .live_session
            .last_reported_total_tokens
            .into_inner(),
    );

    running_entry.live_session.last_reported_input_tokens = token_count(input_tokens);
    running_entry.live_session.last_reported_output_tokens = token_count(output_tokens);
    running_entry.live_session.last_reported_total_tokens = token_count(total_tokens);

    running_entry.live_session.codex_input_tokens = token_count(input_tokens);
    running_entry.live_session.codex_output_tokens = token_count(output_tokens);
    running_entry.live_session.codex_total_tokens = token_count(total_tokens);

    runtime.codex_totals.input_tokens =
        token_count(runtime.codex_totals.input_tokens.into_inner() + delta_input);
    runtime.codex_totals.output_tokens =
        token_count(runtime.codex_totals.output_tokens.into_inner() + delta_output);
    runtime.codex_totals.total_tokens =
        token_count(runtime.codex_totals.total_tokens.into_inner() + delta_total);
}

pub fn continuation_delay_ms() -> u64 {
    1_000
}

pub fn failure_backoff_ms(attempt: u32, max_backoff_ms: u64) -> u64 {
    let exponent = attempt.saturating_sub(1).min(31);
    let raw = 10_000_u64.saturating_mul(2_u64.saturating_pow(exponent));
    raw.min(max_backoff_ms)
}

pub fn parse_state_set(states: &[IssueStateName]) -> HashSet<String> {
    states
        .iter()
        .map(IssueStateName::value)
        .map(normalize_state_name)
        .collect()
}

pub fn parse_running_state_count(runtime: &RuntimeState) -> HashMap<String, u32> {
    let mut counts = HashMap::<String, u32>::new();
    for state in runtime
        .running
        .values()
        .map(|entry| entry.issue.state.value())
    {
        let normalized = normalize_state_name(state);
        counts
            .entry(normalized)
            .and_modify(|value| *value += 1)
            .or_insert(1);
    }

    counts
}

fn within_state_concurrency_limit(
    issue: &Issue,
    runtime: &RuntimeState,
    policy: &DispatchPolicy,
) -> bool {
    let normalized_state = normalize_state_name(issue.state.value());
    let current_state_count = parse_running_state_count(runtime)
        .get(&normalized_state)
        .copied()
        .unwrap_or(0);

    let limit = policy
        .agent
        .max_concurrent_agents_by_state
        .get(&normalized_state)
        .copied()
        .unwrap_or(policy.agent.max_concurrent_agents);

    current_state_count < limit.into_inner()
}

fn has_non_terminal_blocker(issue: &Issue, policy: &DispatchPolicy) -> bool {
    issue.blocked_by.iter().any(|blocker| {
        let Some(blocker_state) = blocker.state.as_ref() else {
            return true;
        };
        !policy
            .terminal_states
            .contains(&normalize_state_name(blocker_state.value()))
    })
}

fn compare_issue_priority(left: &Issue, right: &Issue) -> Ordering {
    let left_priority = left.priority.unwrap_or(i32::MAX);
    let right_priority = right.priority.unwrap_or(i32::MAX);

    left_priority
        .cmp(&right_priority)
        .then_with(|| left.created_at.cmp(&right.created_at))
        .then_with(|| left.identifier.value().cmp(right.identifier.value()))
}

pub fn normalized_state(state: &IssueStateName) -> Result<NormalizedState, String> {
    NormalizedState::try_new(normalize_state_name(state.value())).map_err(|err| err.to_string())
}

fn token_count(value: u64) -> symphony_domain::TokenCount {
    symphony_domain::TokenCount::try_new(value).expect("u64 should map to a valid token count")
}
