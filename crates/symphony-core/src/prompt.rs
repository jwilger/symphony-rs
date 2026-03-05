use std::collections::HashSet;

use liquid::ParserBuilder;
use regex::Regex;
use serde::Serialize;
use serde_json::Value;

use crate::error::PromptError;
use symphony_domain::Issue;

pub fn render_issue_prompt(
    template: &str,
    issue: &Issue,
    attempt: Option<u32>,
) -> Result<String, PromptError> {
    let parser = ParserBuilder::with_stdlib()
        .build()
        .map_err(|err| PromptError::TemplateParseError(err.to_string()))?;
    let compiled = parser
        .parse(template)
        .map_err(|err| PromptError::TemplateParseError(err.to_string()))?;

    let context = serde_json::json!({
        "issue": issue,
        "attempt": attempt,
    });

    validate_template_variables(template, &context)?;

    let runtime_object = liquid::object!({
        "issue": to_liquid_value(issue)?,
        "attempt": to_liquid_value(&attempt)?,
    });

    compiled
        .render(&runtime_object)
        .map(|rendered| rendered.trim().to_string())
        .map_err(|err| PromptError::TemplateRenderError(err.to_string()))
}

fn to_liquid_value<T: Serialize>(value: &T) -> Result<liquid::model::Value, PromptError> {
    liquid::model::to_value(value).map_err(|err| PromptError::TemplateRenderError(err.to_string()))
}

fn validate_template_variables(template: &str, context: &Value) -> Result<(), PromptError> {
    let expression_regex = Regex::new(r"\{\{\s*([a-zA-Z_][a-zA-Z0-9_.]*)[\s|}]?")
        .map_err(|err| PromptError::TemplateParseError(err.to_string()))?;
    let for_loop_regex = Regex::new(r"\{%\s*for\s+\w+\s+in\s+([a-zA-Z_][a-zA-Z0-9_.]*)\s*%\}")
        .map_err(|err| PromptError::TemplateParseError(err.to_string()))?;

    let mut required_paths = HashSet::new();
    for capture in expression_regex.captures_iter(template) {
        if let Some(path) = capture.get(1) {
            required_paths.insert(path.as_str().to_string());
        }
    }

    for capture in for_loop_regex.captures_iter(template) {
        if let Some(path) = capture.get(1) {
            required_paths.insert(path.as_str().to_string());
        }
    }

    for required_path in required_paths {
        if !json_path_exists(context, &required_path) {
            return Err(PromptError::TemplateRenderError(format!(
                "unknown variable: {required_path}"
            )));
        }
    }

    Ok(())
}

fn json_path_exists(context: &Value, dot_path: &str) -> bool {
    let mut current = context;
    for segment in dot_path.split('.') {
        match current {
            Value::Object(object) => {
                if let Some(next) = object.get(segment) {
                    current = next;
                } else {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}
