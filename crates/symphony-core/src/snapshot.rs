use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use symphony_domain::{RetryEntry, RuntimeState};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeCounts {
    pub running: usize,
    pub retrying: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenSnapshot {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunningSnapshotRow {
    pub issue_id: String,
    pub issue_identifier: String,
    pub state: String,
    pub session_id: Option<String>,
    pub turn_count: u32,
    pub last_event: Option<String>,
    pub last_message: Option<String>,
    pub started_at: DateTime<Utc>,
    pub last_event_at: Option<DateTime<Utc>>,
    pub tokens: TokenSnapshot,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RetrySnapshotRow {
    pub issue_id: String,
    pub issue_identifier: String,
    pub attempt: u32,
    pub due_at: DateTime<Utc>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeSnapshot {
    pub generated_at: DateTime<Utc>,
    pub counts: RuntimeCounts,
    pub running: Vec<RunningSnapshotRow>,
    pub retrying: Vec<RetrySnapshotRow>,
    pub codex_totals: CodexTotalsSnapshot,
    pub rate_limits: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CodexTotalsSnapshot {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub seconds_running: f64,
}

pub fn build_runtime_snapshot(runtime: &RuntimeState, now: DateTime<Utc>) -> RuntimeSnapshot {
    let running = runtime
        .running
        .values()
        .map(|entry| RunningSnapshotRow {
            issue_id: entry.issue.id.value().to_string(),
            issue_identifier: entry.issue.identifier.value().to_string(),
            state: entry.issue.state.value().to_string(),
            session_id: entry
                .live_session
                .session_id
                .as_ref()
                .map(|value| value.value().to_string()),
            turn_count: entry.live_session.turn_count.value(),
            last_event: entry.live_session.last_codex_event.clone(),
            last_message: entry.live_session.last_codex_message.clone(),
            started_at: entry.started_at,
            last_event_at: entry.live_session.last_codex_timestamp,
            tokens: TokenSnapshot {
                input_tokens: entry.live_session.codex_input_tokens.value(),
                output_tokens: entry.live_session.codex_output_tokens.value(),
                total_tokens: entry.live_session.codex_total_tokens.value(),
            },
        })
        .collect::<Vec<_>>();

    let retrying = runtime
        .retry_attempts
        .values()
        .map(retry_row)
        .collect::<Vec<_>>();

    let live_elapsed_seconds = runtime
        .running
        .values()
        .map(|entry| {
            now.signed_duration_since(entry.started_at)
                .num_seconds()
                .max(0) as f64
        })
        .sum::<f64>();

    RuntimeSnapshot {
        generated_at: now,
        counts: RuntimeCounts {
            running: running.len(),
            retrying: retrying.len(),
        },
        running,
        retrying,
        codex_totals: CodexTotalsSnapshot {
            input_tokens: runtime.codex_totals.input_tokens.value(),
            output_tokens: runtime.codex_totals.output_tokens.value(),
            total_tokens: runtime.codex_totals.total_tokens.value(),
            seconds_running: runtime.codex_totals.seconds_running as f64 + live_elapsed_seconds,
        },
        rate_limits: runtime.codex_rate_limits.clone(),
    }
}

fn retry_row(entry: &RetryEntry) -> RetrySnapshotRow {
    let due_at = DateTime::from_timestamp_millis(entry.due_at_ms as i64).unwrap_or_else(Utc::now);
    RetrySnapshotRow {
        issue_id: entry.issue_id.value().to_string(),
        issue_identifier: entry.identifier.value().to_string(),
        attempt: entry.attempt,
        due_at,
        error: entry.error.clone(),
    }
}
