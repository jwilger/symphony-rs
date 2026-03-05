use std::collections::{HashMap, HashSet};

use chrono::{Duration, Utc};
use symphony_core::build_runtime_snapshot;
use symphony_domain::{
    CodexTotals, LiveSession, RetryEntry, RunningEntry, RuntimeState, SessionId, ThreadId,
    TokenCount, TurnCount, TurnId, parse_issue_id, parse_issue_identifier, parse_issue_state,
    parse_issue_title,
};

fn sample_issue(identifier: &str, state: &str) -> symphony_domain::Issue {
    symphony_domain::Issue {
        id: parse_issue_id(&format!("id-{identifier}")).expect("issue id should parse"),
        identifier: parse_issue_identifier(identifier).expect("issue identifier should parse"),
        title: parse_issue_title("Snapshot issue").expect("issue title should parse"),
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

#[test]
fn runtime_snapshot_exposes_live_session_fields_and_elapsed_seconds() {
    let now = Utc::now();
    let started_at = now - Duration::seconds(5);
    let issue = sample_issue("SNAP-1", "In Progress");
    let issue_id = issue.id.clone();

    let live_session = LiveSession {
        session_id: Some(SessionId::try_new("session-1".to_string()).expect("session id")),
        thread_id: Some(ThreadId::try_new("thread-1".to_string()).expect("thread id")),
        turn_id: Some(TurnId::try_new("turn-1".to_string()).expect("turn id")),
        codex_app_server_pid: Some("123".to_string()),
        last_codex_event: Some("turn_completed".to_string()),
        last_codex_timestamp: Some(now),
        last_codex_message: Some("done".to_string()),
        codex_input_tokens: TokenCount::try_new(120).expect("token count"),
        codex_output_tokens: TokenCount::try_new(80).expect("token count"),
        codex_total_tokens: TokenCount::try_new(200).expect("token count"),
        last_reported_input_tokens: TokenCount::try_new(120).expect("token count"),
        last_reported_output_tokens: TokenCount::try_new(80).expect("token count"),
        last_reported_total_tokens: TokenCount::try_new(200).expect("token count"),
        turn_count: TurnCount::try_new(2).expect("turn count"),
    };

    let runtime = RuntimeState {
        poll_interval_ms: 30_000,
        max_concurrent_agents: 2,
        running: HashMap::from([(
            issue_id.clone(),
            RunningEntry {
                issue,
                retry_attempt: Some(1),
                started_at,
                live_session,
            },
        )]),
        claimed: HashSet::from([issue_id.clone()]),
        retry_attempts: HashMap::from([(
            issue_id,
            RetryEntry {
                issue_id: parse_issue_id("retry-1").expect("issue id should parse"),
                identifier: parse_issue_identifier("SNAP-RETRY")
                    .expect("issue identifier should parse"),
                attempt: 3,
                due_at_ms: now.timestamp_millis() as u64 + 10_000,
                error: Some("turn_timeout".to_string()),
            },
        )]),
        completed: HashSet::new(),
        codex_totals: CodexTotals {
            input_tokens: TokenCount::try_new(120).expect("token count"),
            output_tokens: TokenCount::try_new(80).expect("token count"),
            total_tokens: TokenCount::try_new(200).expect("token count"),
            seconds_running: 7,
        },
        codex_rate_limits: Some(serde_json::json!({"requests_remaining": 42})),
    };

    let snapshot = build_runtime_snapshot(&runtime, now);

    assert_eq!(snapshot.counts.running, 1);
    assert_eq!(snapshot.counts.retrying, 1);
    assert_eq!(snapshot.running[0].issue_identifier, "SNAP-1");
    assert_eq!(snapshot.running[0].session_id.as_deref(), Some("session-1"));
    assert_eq!(snapshot.running[0].turn_count, 2);
    assert_eq!(snapshot.running[0].tokens.input_tokens, 120);
    assert_eq!(snapshot.running[0].tokens.output_tokens, 80);
    assert_eq!(snapshot.running[0].tokens.total_tokens, 200);
    assert_eq!(snapshot.retrying[0].issue_identifier, "SNAP-RETRY");
    assert_eq!(snapshot.retrying[0].attempt, 3);
    assert_eq!(snapshot.retrying[0].error.as_deref(), Some("turn_timeout"));
    assert_eq!(
        snapshot.rate_limits,
        Some(serde_json::json!({"requests_remaining": 42}))
    );
    assert_eq!(snapshot.codex_totals.seconds_running, 12.0);
}
