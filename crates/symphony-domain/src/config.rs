use std::collections::{BTreeMap, HashMap};

use nutype::nutype;
use serde::{Deserialize, Serialize};

use crate::error::{DomainError, validation_to_domain_error};
use crate::issue::{IssueStateName, parse_issue_state};
use crate::normalization::parse_comma_separated;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackerKind {
    Linear,
}

#[nutype(
    validate(not_empty),
    derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, AsRef)
)]
pub struct TrackerEndpoint(String);

#[nutype(
    validate(not_empty),
    derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, AsRef)
)]
pub struct TrackerApiKey(String);

#[nutype(
    validate(not_empty),
    derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, AsRef)
)]
pub struct ProjectSlug(String);

#[nutype(
    validate(not_empty),
    derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, AsRef)
)]
pub struct CodexCommand(String);

#[nutype(
    validate(not_empty),
    derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, AsRef)
)]
pub struct ApprovalPolicy(String);

#[nutype(
    validate(not_empty),
    derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, AsRef)
)]
pub struct ThreadSandbox(String);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnSandboxPolicy {
    pub json: serde_json::Value,
}

#[nutype(
    validate(greater_or_equal = 1),
    derive(
        Clone,
        Copy,
        Debug,
        PartialEq,
        Eq,
        Hash,
        Ord,
        PartialOrd,
        Serialize,
        Deserialize
    )
)]
pub struct PositiveMs(u64);

#[nutype(
    validate(greater_or_equal = 1),
    derive(
        Clone,
        Copy,
        Debug,
        PartialEq,
        Eq,
        Hash,
        Ord,
        PartialOrd,
        Serialize,
        Deserialize
    )
)]
pub struct PositiveCount(u32);

#[nutype(
    validate(not_empty),
    derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, AsRef)
)]
pub struct WorkspaceRoot(String);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnvResolvedValue {
    Literal(String),
    EnvironmentVariable(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackerConfig {
    pub kind: TrackerKind,
    pub endpoint: TrackerEndpoint,
    pub api_key: TrackerApiKey,
    pub project_slug: ProjectSlug,
    pub active_states: Vec<IssueStateName>,
    pub terminal_states: Vec<IssueStateName>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollingConfig {
    pub interval_ms: PositiveMs,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub root: WorkspaceRoot,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookConfig {
    pub after_create: Option<String>,
    pub before_run: Option<String>,
    pub after_run: Option<String>,
    pub before_remove: Option<String>,
    pub timeout_ms: PositiveMs,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConfig {
    pub max_concurrent_agents: PositiveCount,
    pub max_turns: PositiveCount,
    pub max_retry_backoff_ms: PositiveMs,
    pub max_concurrent_agents_by_state: HashMap<String, PositiveCount>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexConfig {
    pub command: CodexCommand,
    pub approval_policy: ApprovalPolicy,
    pub thread_sandbox: ThreadSandbox,
    pub turn_sandbox_policy: TurnSandboxPolicy,
    pub turn_timeout_ms: PositiveMs,
    pub read_timeout_ms: PositiveMs,
    pub stall_timeout_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerConfig {
    pub port: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceConfig {
    pub tracker: TrackerConfig,
    pub polling: PollingConfig,
    pub workspace: WorkspaceConfig,
    pub hooks: HookConfig,
    pub agent: AgentConfig,
    pub codex: CodexConfig,
    pub server: Option<ServerConfig>,
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl TrackerApiKey {
    pub fn value(&self) -> &str {
        self.as_ref()
    }
}

impl ProjectSlug {
    pub fn value(&self) -> &str {
        self.as_ref()
    }
}

impl CodexCommand {
    pub fn value(&self) -> &str {
        self.as_ref()
    }
}

impl ApprovalPolicy {
    pub fn value(&self) -> &str {
        self.as_ref()
    }
}

impl WorkspaceRoot {
    pub fn value(&self) -> &str {
        self.as_ref()
    }
}

impl PositiveMs {
    pub fn value(&self) -> u64 {
        self.into_inner()
    }
}

impl PositiveCount {
    pub fn value(&self) -> u32 {
        self.into_inner()
    }
}

pub fn parse_positive_ms(
    field: &'static str,
    raw: i64,
    default: u64,
) -> Result<PositiveMs, DomainError> {
    let candidate = if raw <= 0 { default } else { raw as u64 };
    PositiveMs::try_new(candidate).map_err(|err| validation_to_domain_error(field, err))
}

pub fn parse_positive_count(
    field: &'static str,
    raw: i64,
    default: u32,
) -> Result<PositiveCount, DomainError> {
    let candidate = if raw <= 0 { default } else { raw as u32 };
    PositiveCount::try_new(candidate).map_err(|err| validation_to_domain_error(field, err))
}

pub fn parse_workspace_root(raw: &str) -> Result<WorkspaceRoot, DomainError> {
    WorkspaceRoot::try_new(raw.trim().to_string())
        .map_err(|err| validation_to_domain_error("workspace.root", err))
}

pub fn parse_state_list(raw: &str) -> Result<Vec<IssueStateName>, DomainError> {
    let parsed = parse_comma_separated(raw)
        .into_iter()
        .map(|segment| parse_issue_state(&segment))
        .collect::<Result<Vec<_>, _>>()?;

    if parsed.is_empty() {
        return Err(DomainError::invalid(
            "tracker.states",
            "at least one state is required",
        ));
    }

    Ok(parsed)
}

pub fn parse_env_resolved_value(raw: &str) -> Result<EnvResolvedValue, DomainError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(DomainError::invalid("env", "value cannot be empty"));
    }

    if let Some(environment_variable) = trimmed.strip_prefix('$') {
        if environment_variable.is_empty() {
            return Err(DomainError::invalid(
                "env",
                "environment variable name is missing",
            ));
        }
        return Ok(EnvResolvedValue::EnvironmentVariable(
            environment_variable.to_string(),
        ));
    }

    Ok(EnvResolvedValue::Literal(trimmed.to_string()))
}
