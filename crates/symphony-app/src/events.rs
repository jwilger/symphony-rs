use chrono::{DateTime, Utc};

use symphony_core::WorkerExitReason;
use symphony_domain::{IssueId, ServiceConfig, WorkflowDefinition};

#[derive(Debug, Clone)]
pub enum ServiceEvent {
    WorkflowReloaded {
        workflow: WorkflowDefinition,
        config: ServiceConfig,
    },
    WorkerExited {
        issue_id: IssueId,
        reason: WorkerExitReason,
    },
    CodexTokenTotals {
        issue_id: IssueId,
        input_tokens: u64,
        output_tokens: u64,
        total_tokens: u64,
    },
    CodexActivity {
        issue_id: IssueId,
        event: String,
        message: Option<String>,
        at: DateTime<Utc>,
    },
    CodexRateLimits {
        payload: serde_json::Value,
    },
    ForceRefresh,
}
