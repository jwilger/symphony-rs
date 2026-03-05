use std::collections::BTreeMap;

use serde_json::Value;

use crate::error::WorkflowError;
use symphony_domain::WorkflowDefinition;

pub fn parse_workflow(contents: &str) -> Result<WorkflowDefinition, WorkflowError> {
    let trimmed = contents.trim_end_matches('\n');
    if !trimmed.starts_with("---") {
        return Ok(WorkflowDefinition {
            config: BTreeMap::new(),
            prompt_template: trimmed.trim().to_string(),
        });
    }

    let mut lines = trimmed.lines();
    let Some(first_line) = lines.next() else {
        return Ok(WorkflowDefinition::empty());
    };

    if first_line.trim() != "---" {
        return Ok(WorkflowDefinition {
            config: BTreeMap::new(),
            prompt_template: trimmed.trim().to_string(),
        });
    }

    let mut yaml_lines = Vec::new();
    let mut found_front_matter_end = false;
    for line in lines.by_ref() {
        if line.trim() == "---" {
            found_front_matter_end = true;
            break;
        }
        yaml_lines.push(line);
    }

    if !found_front_matter_end {
        return Err(WorkflowError::WorkflowParseError(
            "front matter start marker found without closing marker".to_string(),
        ));
    }

    let yaml_payload = yaml_lines.join("\n");
    let front_matter_value: serde_yaml::Value =
        serde_yaml::from_str(&yaml_payload).map_err(|err| {
            WorkflowError::WorkflowParseError(format!("failed to decode yaml front matter: {err}"))
        })?;

    let object = front_matter_value
        .as_mapping()
        .ok_or(WorkflowError::WorkflowFrontMatterNotAMap)?;

    let config = object
        .iter()
        .map(|(key, value)| {
            let key_string = key
                .as_str()
                .ok_or_else(|| {
                    WorkflowError::WorkflowParseError(
                        "front matter keys must be strings".to_string(),
                    )
                })?
                .to_string();
            let value_json = serde_json::to_value(value).map_err(|err| {
                WorkflowError::WorkflowParseError(format!("invalid front matter value: {err}"))
            })?;
            Ok::<(String, Value), WorkflowError>((key_string, value_json))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;

    let prompt_body = lines.collect::<Vec<_>>().join("\n").trim().to_string();

    Ok(WorkflowDefinition {
        config,
        prompt_template: prompt_body,
    })
}
