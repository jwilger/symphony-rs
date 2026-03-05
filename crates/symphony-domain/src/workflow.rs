use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub config: BTreeMap<String, Value>,
    pub prompt_template: String,
}

impl WorkflowDefinition {
    pub fn empty() -> Self {
        Self {
            config: BTreeMap::new(),
            prompt_template: String::new(),
        }
    }
}
