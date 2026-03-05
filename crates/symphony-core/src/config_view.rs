use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use serde_json::Value;

use crate::error::ConfigError;
use symphony_domain::{
    AgentConfig, ApprovalPolicy, CodexCommand, CodexConfig, EnvResolvedValue, HookConfig,
    PollingConfig, PositiveMs, ProjectSlug, ServerConfig, ServiceConfig, ThreadSandbox,
    TrackerApiKey, TrackerConfig, TrackerEndpoint, TrackerKind, WorkflowDefinition,
    WorkspaceConfig, parse_env_resolved_value, parse_issue_state, parse_positive_count,
    parse_positive_ms, parse_state_list, parse_workspace_root,
};

const DEFAULT_LINEAR_ENDPOINT: &str = "https://api.linear.app/graphql";
const DEFAULT_ACTIVE_STATES: &str = "Todo, In Progress";
const DEFAULT_TERMINAL_STATES: &str = "Closed, Cancelled, Canceled, Duplicate, Done";
const DEFAULT_POLL_INTERVAL_MS: u64 = 30_000;
const DEFAULT_HOOK_TIMEOUT_MS: u64 = 60_000;
const DEFAULT_MAX_CONCURRENT_AGENTS: u32 = 10;
const DEFAULT_MAX_TURNS: u32 = 20;
const DEFAULT_MAX_RETRY_BACKOFF_MS: u64 = 300_000;
const DEFAULT_CODEX_COMMAND: &str = "codex app-server";
const DEFAULT_APPROVAL_POLICY: &str = "never";
const DEFAULT_THREAD_SANDBOX: &str = "danger-full-access";
const DEFAULT_TURN_TIMEOUT_MS: u64 = 3_600_000;
const DEFAULT_READ_TIMEOUT_MS: u64 = 5_000;
const DEFAULT_STALL_TIMEOUT_MS: i64 = 300_000;

pub fn build_service_config(
    workflow: &WorkflowDefinition,
    environment: &HashMap<String, String>,
) -> Result<ServiceConfig, ConfigError> {
    let tracker_object = read_object(&workflow.config, "tracker");
    let kind_raw = read_string(&tracker_object, "kind").unwrap_or_else(|| "linear".to_string());
    let kind = match kind_raw.trim().to_lowercase().as_str() {
        "linear" => TrackerKind::Linear,
        _ => return Err(ConfigError::UnsupportedTrackerKind),
    };

    let endpoint_raw = read_string(&tracker_object, "endpoint")
        .unwrap_or_else(|| DEFAULT_LINEAR_ENDPOINT.to_string());
    let endpoint = TrackerEndpoint::try_new(endpoint_raw)
        .map_err(|err| ConfigError::InvalidConfig(err.to_string()))?;

    let api_key_raw =
        read_string(&tracker_object, "api_key").unwrap_or_else(|| "$LINEAR_API_KEY".to_string());
    let api_key = resolve_env_string(&api_key_raw, environment)
        .ok_or(ConfigError::MissingTrackerApiKey)
        .and_then(|value| {
            TrackerApiKey::try_new(value).map_err(|err| ConfigError::InvalidConfig(err.to_string()))
        })?;

    let project_slug_raw = read_string(&tracker_object, "project_slug")
        .ok_or(ConfigError::MissingTrackerProjectSlug)?;
    let project_slug = resolve_env_string(&project_slug_raw, environment)
        .ok_or(ConfigError::MissingTrackerProjectSlug)
        .and_then(|value| {
            ProjectSlug::try_new(value).map_err(|err| ConfigError::InvalidConfig(err.to_string()))
        })?;

    let active_states = parse_issue_states(
        read_value(&tracker_object, "active_states")
            .unwrap_or(Value::String(DEFAULT_ACTIVE_STATES.to_string())),
    )?;
    let terminal_states = parse_issue_states(
        read_value(&tracker_object, "terminal_states")
            .unwrap_or(Value::String(DEFAULT_TERMINAL_STATES.to_string())),
    )?;

    let tracker = TrackerConfig {
        kind,
        endpoint,
        api_key,
        project_slug,
        active_states,
        terminal_states,
    };

    let polling_object = read_object(&workflow.config, "polling");
    let poll_interval_ms = parse_positive_ms(
        "polling.interval_ms",
        read_i64(&polling_object, "interval_ms").unwrap_or(DEFAULT_POLL_INTERVAL_MS as i64),
        DEFAULT_POLL_INTERVAL_MS,
    )
    .map_err(|err| ConfigError::InvalidConfig(err.to_string()))?;
    let polling = PollingConfig {
        interval_ms: poll_interval_ms,
    };

    let workspace_object = read_object(&workflow.config, "workspace");
    let default_workspace_root = std::env::temp_dir()
        .join("symphony_workspaces")
        .to_string_lossy()
        .to_string();
    let workspace_root_raw =
        read_string(&workspace_object, "root").unwrap_or(default_workspace_root);
    let workspace_root_expanded = expand_workspace_root(&workspace_root_raw, environment);
    let workspace_root = parse_workspace_root(&workspace_root_expanded)
        .map_err(|err| ConfigError::InvalidConfig(err.to_string()))?;
    let workspace = WorkspaceConfig {
        root: workspace_root,
    };

    let hook_object = read_object(&workflow.config, "hooks");
    let hooks = HookConfig {
        after_create: read_string(&hook_object, "after_create"),
        before_run: read_string(&hook_object, "before_run"),
        after_run: read_string(&hook_object, "after_run"),
        before_remove: read_string(&hook_object, "before_remove"),
        timeout_ms: parse_positive_ms(
            "hooks.timeout_ms",
            read_i64(&hook_object, "timeout_ms").unwrap_or(DEFAULT_HOOK_TIMEOUT_MS as i64),
            DEFAULT_HOOK_TIMEOUT_MS,
        )
        .map_err(|err| ConfigError::InvalidConfig(err.to_string()))?,
    };

    let agent_object = read_object(&workflow.config, "agent");
    let per_state_limits = read_value(&agent_object, "max_concurrent_agents_by_state")
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    let max_concurrent_agents_by_state = per_state_limits
        .into_iter()
        .filter_map(|(state, value)| {
            value
                .as_i64()
                .and_then(|raw| {
                    parse_positive_count("agent.max_concurrent_agents_by_state", raw, 0).ok()
                })
                .map(|parsed| (state.trim().to_lowercase(), parsed))
        })
        .collect();

    let agent = AgentConfig {
        max_concurrent_agents: parse_positive_count(
            "agent.max_concurrent_agents",
            read_i64(&agent_object, "max_concurrent_agents")
                .unwrap_or(DEFAULT_MAX_CONCURRENT_AGENTS as i64),
            DEFAULT_MAX_CONCURRENT_AGENTS,
        )
        .map_err(|err| ConfigError::InvalidConfig(err.to_string()))?,
        max_turns: parse_positive_count(
            "agent.max_turns",
            read_i64(&agent_object, "max_turns").unwrap_or(DEFAULT_MAX_TURNS as i64),
            DEFAULT_MAX_TURNS,
        )
        .map_err(|err| ConfigError::InvalidConfig(err.to_string()))?,
        max_retry_backoff_ms: parse_positive_ms(
            "agent.max_retry_backoff_ms",
            read_i64(&agent_object, "max_retry_backoff_ms")
                .unwrap_or(DEFAULT_MAX_RETRY_BACKOFF_MS as i64),
            DEFAULT_MAX_RETRY_BACKOFF_MS,
        )
        .map_err(|err| ConfigError::InvalidConfig(err.to_string()))?,
        max_concurrent_agents_by_state,
    };

    let codex_object = read_object(&workflow.config, "codex");
    let command = CodexCommand::try_new(
        read_string(&codex_object, "command").unwrap_or_else(|| DEFAULT_CODEX_COMMAND.to_string()),
    )
    .map_err(|err| ConfigError::InvalidConfig(err.to_string()))?;
    let approval_policy = ApprovalPolicy::try_new(
        read_string(&codex_object, "approval_policy")
            .unwrap_or_else(|| DEFAULT_APPROVAL_POLICY.to_string()),
    )
    .map_err(|err| ConfigError::InvalidConfig(err.to_string()))?;
    let thread_sandbox = ThreadSandbox::try_new(
        read_string(&codex_object, "thread_sandbox")
            .unwrap_or_else(|| DEFAULT_THREAD_SANDBOX.to_string()),
    )
    .map_err(|err| ConfigError::InvalidConfig(err.to_string()))?;

    let sandbox_policy = read_value(&codex_object, "turn_sandbox_policy").unwrap_or_else(|| {
        serde_json::json!({
            "type": "dangerFullAccess"
        })
    });

    let codex = CodexConfig {
        command,
        approval_policy,
        thread_sandbox,
        turn_sandbox_policy: symphony_domain::TurnSandboxPolicy {
            json: sandbox_policy,
        },
        turn_timeout_ms: parse_positive_ms(
            "codex.turn_timeout_ms",
            read_i64(&codex_object, "turn_timeout_ms").unwrap_or(DEFAULT_TURN_TIMEOUT_MS as i64),
            DEFAULT_TURN_TIMEOUT_MS,
        )
        .map_err(|err| ConfigError::InvalidConfig(err.to_string()))?,
        read_timeout_ms: parse_positive_ms(
            "codex.read_timeout_ms",
            read_i64(&codex_object, "read_timeout_ms").unwrap_or(DEFAULT_READ_TIMEOUT_MS as i64),
            DEFAULT_READ_TIMEOUT_MS,
        )
        .map_err(|err| ConfigError::InvalidConfig(err.to_string()))?,
        stall_timeout_ms: read_i64(&codex_object, "stall_timeout_ms")
            .unwrap_or(DEFAULT_STALL_TIMEOUT_MS),
    };

    let server = read_object(&workflow.config, "server")
        .get("port")
        .and_then(Value::as_u64)
        .map(|port| ServerConfig { port: port as u16 });

    let ignored_extensions = workflow
        .config
        .iter()
        .filter(|(key, _)| {
            ![
                "tracker",
                "polling",
                "workspace",
                "hooks",
                "agent",
                "codex",
                "server",
            ]
            .contains(&key.as_str())
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<BTreeMap<_, _>>();

    Ok(ServiceConfig {
        tracker,
        polling,
        workspace,
        hooks,
        agent,
        codex,
        server,
        extra: ignored_extensions,
    })
}

pub fn validate_dispatch_config(config: &ServiceConfig) -> Result<(), ConfigError> {
    if config.tracker.api_key.value().trim().is_empty() {
        return Err(ConfigError::MissingTrackerApiKey);
    }

    if config.tracker.project_slug.value().trim().is_empty() {
        return Err(ConfigError::MissingTrackerProjectSlug);
    }

    if config.codex.command.value().trim().is_empty() {
        return Err(ConfigError::InvalidConfig(
            "codex.command is required".to_string(),
        ));
    }

    Ok(())
}

fn read_object(root: &BTreeMap<String, Value>, key: &str) -> serde_json::Map<String, Value> {
    root.get(key)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

fn read_value(object: &serde_json::Map<String, Value>, key: &str) -> Option<Value> {
    object.get(key).cloned()
}

fn read_string(object: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    object.get(key).and_then(|value| match value {
        Value::String(text) => Some(text.clone()),
        _ => None,
    })
}

fn read_i64(object: &serde_json::Map<String, Value>, key: &str) -> Option<i64> {
    object.get(key).and_then(|value| match value {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.parse::<i64>().ok(),
        _ => None,
    })
}

fn parse_issue_states(value: Value) -> Result<Vec<symphony_domain::IssueStateName>, ConfigError> {
    match value {
        Value::String(text) => {
            parse_state_list(&text).map_err(|err| ConfigError::InvalidConfig(err.to_string()))
        }
        Value::Array(items) => {
            let mut parsed = Vec::new();
            for item in items {
                let Some(raw) = item.as_str() else {
                    continue;
                };
                let state = parse_issue_state(raw)
                    .map_err(|err| ConfigError::InvalidConfig(err.to_string()))?;
                parsed.push(state);
            }
            if parsed.is_empty() {
                Err(ConfigError::InvalidConfig(
                    "state lists must include at least one item".to_string(),
                ))
            } else {
                Ok(parsed)
            }
        }
        _ => Err(ConfigError::InvalidConfig(
            "state list must be a string or array".to_string(),
        )),
    }
}

fn resolve_env_string(raw: &str, environment: &HashMap<String, String>) -> Option<String> {
    match parse_env_resolved_value(raw).ok()? {
        EnvResolvedValue::Literal(value) => Some(value),
        EnvResolvedValue::EnvironmentVariable(variable_name) => environment
            .get(&variable_name)
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
    }
}

fn expand_workspace_root(raw: &str, environment: &HashMap<String, String>) -> String {
    let resolved = resolve_env_string(raw, environment).unwrap_or_else(|| raw.to_string());
    if let Some(stripped) = resolved.strip_prefix("~/") {
        std::env::var("HOME")
            .map(|home| {
                PathBuf::from(home)
                    .join(stripped)
                    .to_string_lossy()
                    .to_string()
            })
            .unwrap_or(resolved.clone())
    } else {
        resolved.clone()
    }
}

pub fn positive_ms_to_u64(value: PositiveMs) -> u64 {
    value.into_inner()
}
