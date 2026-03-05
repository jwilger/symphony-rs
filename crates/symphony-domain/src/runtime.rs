use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use nutype::nutype;
use serde::{Deserialize, Serialize};

use crate::issue::{Issue, IssueId, IssueIdentifier};

#[nutype(
    validate(not_empty),
    derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, AsRef)
)]
pub struct SessionId(String);

#[nutype(
    validate(not_empty),
    derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, AsRef)
)]
pub struct ThreadId(String);

#[nutype(
    validate(not_empty),
    derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, AsRef)
)]
pub struct TurnId(String);

#[nutype(
    validate(greater_or_equal = 0),
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
pub struct TokenCount(u64);

#[nutype(
    validate(greater_or_equal = 0),
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
pub struct TurnCount(u32);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveSession {
    pub session_id: Option<SessionId>,
    pub thread_id: Option<ThreadId>,
    pub turn_id: Option<TurnId>,
    pub codex_app_server_pid: Option<String>,
    pub last_codex_event: Option<String>,
    pub last_codex_timestamp: Option<DateTime<Utc>>,
    pub last_codex_message: Option<String>,
    pub codex_input_tokens: TokenCount,
    pub codex_output_tokens: TokenCount,
    pub codex_total_tokens: TokenCount,
    pub last_reported_input_tokens: TokenCount,
    pub last_reported_output_tokens: TokenCount,
    pub last_reported_total_tokens: TokenCount,
    pub turn_count: TurnCount,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryEntry {
    pub issue_id: IssueId,
    pub identifier: IssueIdentifier,
    pub attempt: u32,
    pub due_at_ms: u64,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunningEntry {
    pub issue: Issue,
    pub retry_attempt: Option<u32>,
    pub started_at: DateTime<Utc>,
    pub live_session: LiveSession,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexTotals {
    pub input_tokens: TokenCount,
    pub output_tokens: TokenCount,
    pub total_tokens: TokenCount,
    pub seconds_running: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuntimeState {
    pub poll_interval_ms: u64,
    pub max_concurrent_agents: u32,
    pub running: HashMap<IssueId, RunningEntry>,
    pub claimed: HashSet<IssueId>,
    pub retry_attempts: HashMap<IssueId, RetryEntry>,
    pub completed: HashSet<IssueId>,
    pub codex_totals: CodexTotals,
    pub codex_rate_limits: Option<serde_json::Value>,
}

impl SessionId {
    pub fn value(&self) -> &str {
        self.as_ref()
    }
}

impl ThreadId {
    pub fn value(&self) -> &str {
        self.as_ref()
    }
}

impl TurnId {
    pub fn value(&self) -> &str {
        self.as_ref()
    }
}

impl TokenCount {
    pub fn value(&self) -> u64 {
        self.into_inner()
    }
}

impl TurnCount {
    pub fn value(&self) -> u32 {
        self.into_inner()
    }
}

impl RuntimeState {
    pub fn running_count(&self) -> usize {
        self.running.len()
    }
}

impl Default for LiveSession {
    fn default() -> Self {
        Self {
            session_id: None,
            thread_id: None,
            turn_id: None,
            codex_app_server_pid: None,
            last_codex_event: None,
            last_codex_timestamp: None,
            last_codex_message: None,
            codex_input_tokens: zero_token_count(),
            codex_output_tokens: zero_token_count(),
            codex_total_tokens: zero_token_count(),
            last_reported_input_tokens: zero_token_count(),
            last_reported_output_tokens: zero_token_count(),
            last_reported_total_tokens: zero_token_count(),
            turn_count: zero_turn_count(),
        }
    }
}

impl Default for CodexTotals {
    fn default() -> Self {
        Self {
            input_tokens: zero_token_count(),
            output_tokens: zero_token_count(),
            total_tokens: zero_token_count(),
            seconds_running: 0,
        }
    }
}

fn zero_token_count() -> TokenCount {
    TokenCount::try_new(0).expect("0 should be a valid token count")
}

fn zero_turn_count() -> TurnCount {
    TurnCount::try_new(0).expect("0 should be a valid turn count")
}
