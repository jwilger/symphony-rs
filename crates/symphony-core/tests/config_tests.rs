use std::collections::{BTreeMap, HashMap};

use serde_json::json;
use symphony_core::build_service_config;
use symphony_domain::WorkflowDefinition;

#[test]
fn builds_default_config_with_env_resolution() {
    let workflow = WorkflowDefinition {
        config: BTreeMap::from([(
            "tracker".to_string(),
            json!({"kind": "linear", "api_key": "$LINEAR_API_KEY", "project_slug": "APP"}),
        )]),
        prompt_template: "Prompt".to_string(),
    };

    let environment = HashMap::from([("LINEAR_API_KEY".to_string(), "secret".to_string())]);
    let config = build_service_config(&workflow, &environment).expect("config should parse");

    assert_eq!(config.tracker.api_key.value(), "secret");
    assert_eq!(config.polling.interval_ms.into_inner(), 30_000);
    assert_eq!(config.agent.max_retry_backoff_ms.into_inner(), 300_000);
    assert_eq!(config.codex.command.value(), "codex app-server");
    assert_eq!(config.codex.approval_policy.value(), "never");
}

#[test]
fn missing_api_key_fails_validation() {
    let workflow = WorkflowDefinition {
        config: BTreeMap::from([(
            "tracker".to_string(),
            json!({"kind": "linear", "project_slug": "APP"}),
        )]),
        prompt_template: "Prompt".to_string(),
    };

    let config_result = build_service_config(&workflow, &HashMap::new());
    assert!(config_result.is_err());
}

#[test]
fn dispatch_validation_rejects_empty_codex_command() {
    let workflow = WorkflowDefinition {
        config: BTreeMap::from([
            (
                "tracker".to_string(),
                json!({"kind": "linear", "api_key": "abc", "project_slug": "APP"}),
            ),
            ("codex".to_string(), json!({"command": ""})),
        ]),
        prompt_template: "Prompt".to_string(),
    };

    assert!(build_service_config(&workflow, &HashMap::new()).is_err());
}
