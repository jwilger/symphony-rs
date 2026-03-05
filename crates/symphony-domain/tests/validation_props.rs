use proptest::prelude::*;
use symphony_domain::{
    EnvResolvedValue, normalize_state_name, parse_env_resolved_value, parse_issue_state,
    parse_label, parse_positive_count, parse_state_list, sanitize_workspace_key,
};

proptest! {
    #[test]
    fn normalized_states_are_trimmed_and_lowercase(input in ".*") {
        let normalized = normalize_state_name(&input);
        prop_assert_eq!(normalized.as_str(), normalized.trim());
        prop_assert_eq!(normalized.as_str(), normalized.to_lowercase());
    }

    #[test]
    fn sanitized_workspace_keys_only_use_allowed_characters(input in ".*") {
        let sanitized = sanitize_workspace_key(&input);
        prop_assert!(!sanitized.is_empty());
        for character in sanitized.chars() {
            prop_assert!(character.is_ascii_alphanumeric() || character == '.' || character == '_' || character == '-');
        }
    }

    #[test]
    fn env_token_parser_roundtrips_literals_and_env_vars(identifier in "[A-Z_][A-Z0-9_]{0,20}", literal in "[a-zA-Z0-9_\\-./]{1,30}") {
        let env_result = parse_env_resolved_value(&format!("${identifier}"));
        prop_assert!(matches!(env_result, Ok(EnvResolvedValue::EnvironmentVariable(value)) if value == identifier));

        let literal_result = parse_env_resolved_value(&literal);
        prop_assert!(matches!(literal_result, Ok(EnvResolvedValue::Literal(value)) if value == literal));
    }

    #[test]
    fn label_parser_outputs_lowercase_ascii(label in "[A-Za-z0-9._-]{1,40}") {
        let parsed = parse_label(&label);
        prop_assert!(parsed.is_ok());
        let parsed = parsed.expect("parser already checked as ok");
        prop_assert_eq!(parsed.as_ref(), parsed.as_ref().to_lowercase());
    }

    #[test]
    fn issue_state_parser_rejects_whitespace_only(whitespace in "\\s+") {
        prop_assert!(parse_issue_state(&whitespace).is_err());
    }
}

#[test]
fn state_list_parser_keeps_order_and_non_empty_entries() {
    let states = parse_state_list("Todo, In Progress,Done").expect("valid state list");
    let names: Vec<&str> = states.iter().map(|state| state.value()).collect();

    assert_eq!(names, vec!["Todo", "In Progress", "Done"]);
}

#[test]
fn positive_count_parser_uses_default_for_non_positive_values() {
    let parsed = parse_positive_count("agent.max_turns", 0, 7).expect("default should parse");

    assert_eq!(parsed.value(), 7);
}
