use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use symphony_core::{
    DispatchPolicy, WorkerExitReason, apply_worker_exit, continuation_delay_ms, failure_backoff_ms,
    initial_runtime_state, register_running_issue, should_dispatch, sort_for_dispatch,
};
use symphony_domain::{
    AgentConfig, BlockerRef, CodexTotals, Issue, PositiveCount, parse_issue_id,
    parse_issue_identifier, parse_issue_state, parse_issue_title, parse_label,
};

fn make_issue(
    identifier: &str,
    state: &str,
    priority: Option<i32>,
    created_at: Option<DateTime<Utc>>,
) -> Issue {
    Issue {
        id: parse_issue_id(&format!("id-{identifier}"))
            .expect("issue id should parse in test helper"),
        identifier: parse_issue_identifier(identifier)
            .expect("issue identifier should parse in test helper"),
        title: parse_issue_title("Test issue").expect("issue title should parse in test helper"),
        description: None,
        priority,
        state: parse_issue_state(state).expect("issue state should parse in test helper"),
        branch_name: None,
        url: None,
        labels: vec![parse_label("ops").expect("label should parse in test helper")],
        blocked_by: Vec::new(),
        created_at,
        updated_at: created_at,
    }
}

fn dispatch_policy() -> DispatchPolicy {
    DispatchPolicy {
        active_states: HashSet::from(["todo".to_string(), "in progress".to_string()]),
        terminal_states: HashSet::from([
            "done".to_string(),
            "closed".to_string(),
            "cancelled".to_string(),
        ]),
        agent: AgentConfig {
            max_concurrent_agents: PositiveCount::try_new(4)
                .expect("positive count should parse in test helper"),
            max_turns: PositiveCount::try_new(20)
                .expect("positive count should parse in test helper"),
            max_retry_backoff_ms: symphony_domain::PositiveMs::try_new(300_000)
                .expect("positive ms should parse in test helper"),
            max_concurrent_agents_by_state: HashMap::new(),
        },
    }
}

#[test]
fn sorts_by_priority_created_at_then_identifier() {
    let oldest = DateTime::from_timestamp(1, 0).expect("valid timestamp");
    let newer = DateTime::from_timestamp(2, 0).expect("valid timestamp");

    let issues = vec![
        make_issue("ABC-2", "Todo", Some(2), Some(newer)),
        make_issue("ABC-1", "Todo", Some(1), Some(newer)),
        make_issue("ABC-3", "Todo", Some(1), Some(oldest)),
    ];

    let sorted = sort_for_dispatch(&issues);
    let identifiers = sorted
        .iter()
        .map(|issue| issue.identifier.value())
        .collect::<Vec<_>>();

    assert_eq!(identifiers, vec!["ABC-3", "ABC-1", "ABC-2"]);
}

#[test]
fn todo_with_non_terminal_blocker_is_not_dispatchable() {
    let mut issue = make_issue("ABC-4", "Todo", Some(1), None);
    issue.blocked_by.push(BlockerRef {
        id: None,
        identifier: None,
        state: Some(parse_issue_state("In Progress").expect("blocker state should parse in test")),
    });

    let runtime = initial_runtime_state(30_000, 10);
    assert!(!should_dispatch(&issue, &runtime, &dispatch_policy()));
}

#[test]
fn todo_with_terminal_blocker_is_dispatchable() {
    let mut issue = make_issue("ABC-5", "Todo", Some(1), None);
    issue.blocked_by.push(BlockerRef {
        id: None,
        identifier: None,
        state: Some(parse_issue_state("Done").expect("blocker state should parse in test")),
    });

    let runtime = initial_runtime_state(30_000, 10);
    assert!(should_dispatch(&issue, &runtime, &dispatch_policy()));
}

#[test]
fn continuation_retry_uses_one_second_delay() {
    assert_eq!(continuation_delay_ms(), 1_000);
}

#[test]
fn failure_retry_backoff_caps_at_configured_maximum() {
    assert_eq!(failure_backoff_ms(1, 300_000), 10_000);
    assert_eq!(failure_backoff_ms(2, 300_000), 20_000);
    assert_eq!(failure_backoff_ms(6, 300_000), 300_000);
}

#[test]
fn normal_worker_exit_schedules_continuation_attempt() {
    let issue = make_issue("ABC-6", "In Progress", Some(1), None);
    let issue_id = issue.id.clone();
    let mut runtime = initial_runtime_state(30_000, 10);
    register_running_issue(&mut runtime, issue, None, Utc::now());

    let retry_entry = apply_worker_exit(
        &mut runtime,
        &issue_id,
        WorkerExitReason::Normal,
        1_000,
        300_000,
    )
    .expect("running issue should produce retry entry");

    assert_eq!(retry_entry.attempt, 1);
    assert_eq!(retry_entry.due_at_ms, 2_000);
    assert!(runtime.completed.contains(&issue_id));
}

#[test]
fn abnormal_worker_exit_increments_attempt_and_sets_error() {
    let issue = make_issue("ABC-7", "In Progress", Some(1), None);
    let issue_id = issue.id.clone();
    let mut runtime = initial_runtime_state(30_000, 10);
    register_running_issue(&mut runtime, issue, Some(2), Utc::now());

    let retry_entry = apply_worker_exit(
        &mut runtime,
        &issue_id,
        WorkerExitReason::Failed("boom".to_string()),
        0,
        300_000,
    )
    .expect("running issue should produce retry entry");

    assert_eq!(retry_entry.attempt, 3);
    assert_eq!(retry_entry.error.as_deref(), Some("boom"));
}

#[test]
fn initializes_runtime_with_zero_totals() {
    let runtime = initial_runtime_state(30_000, 10);
    assert_eq!(runtime.codex_totals, CodexTotals::default());
}
