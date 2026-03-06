use std::collections::BTreeMap;
use std::path::PathBuf;

use symphony_core::{build_service_config, parse_workflow, render_issue_prompt};
use symphony_domain::{
    Issue, parse_issue_id, parse_issue_identifier, parse_issue_state, parse_issue_title,
    parse_label,
};

fn sample_issue() -> Issue {
    Issue {
        id: parse_issue_id("abc123").expect("issue id should parse in test helper"),
        identifier: parse_issue_identifier("ABC-123")
            .expect("issue identifier should parse in test helper"),
        title: parse_issue_title("Fix orchestrator")
            .expect("issue title should parse in test helper"),
        description: Some("description".to_string()),
        priority: Some(1),
        state: parse_issue_state("In Progress").expect("issue state should parse in test helper"),
        branch_name: Some("feature/abc-123".to_string()),
        url: Some("https://linear.app/issue/ABC-123".to_string()),
        labels: vec![parse_label("backend").expect("label should parse in test helper")],
        blocked_by: Vec::new(),
        created_at: None,
        updated_at: None,
    }
}

#[test]
fn parses_workflow_with_front_matter_map() {
    let workflow = parse_workflow(
        r#"---
tracker:
  kind: linear
  project_slug: APP
---

Hello {{ issue.identifier }}
"#,
    )
    .expect("workflow should parse");

    assert_eq!(
        workflow
            .config
            .get("tracker")
            .expect("tracker key should exist")
            .get("kind")
            .expect("tracker kind should exist"),
        "linear"
    );
    assert_eq!(workflow.prompt_template, "Hello {{ issue.identifier }}");
}

#[test]
fn parses_workflow_without_front_matter() {
    let workflow =
        parse_workflow("Work only on {{ issue.identifier }}").expect("workflow should parse");

    assert_eq!(workflow.config, BTreeMap::new());
    assert_eq!(
        workflow.prompt_template,
        "Work only on {{ issue.identifier }}"
    );
}

#[test]
fn strict_prompt_rendering_fails_for_unknown_variables() {
    let issue = sample_issue();

    let result = render_issue_prompt("Hello {{ unknown_var }}", &issue, None);

    assert!(result.is_err());
}

#[test]
fn prompt_rendering_includes_attempt_when_present() {
    let issue = sample_issue();

    let prompt = render_issue_prompt(
        "Issue {{ issue.identifier }} attempt={{ attempt }}",
        &issue,
        Some(3),
    )
    .expect("prompt should render");

    assert_eq!(prompt, "Issue ABC-123 attempt=3");
}


#[test]
fn repository_workflow_file_parses_for_local_development() {
    let workflow_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("WORKFLOW.md");
    let workflow_contents = std::fs::read_to_string(&workflow_path)
        .expect("repository WORKFLOW.md should exist");
    let workflow = parse_workflow(&workflow_contents).expect("repository workflow should parse");
    let config = build_service_config(&workflow, &std::collections::HashMap::new())
        .expect("repository workflow should build a service config");

    assert_eq!(config.tracker.project_slug.value(), "DEMO");
    assert_eq!(config.server.expect("server config should exist").port, 3000);
}
