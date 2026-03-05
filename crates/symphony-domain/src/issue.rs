use chrono::{DateTime, Utc};
use nutype::nutype;
use serde::{Deserialize, Serialize};

use crate::error::{DomainError, validation_to_domain_error};
use crate::normalization::{normalize_label, normalize_state_name};

#[nutype(
    validate(not_empty),
    derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, AsRef)
)]
pub struct IssueId(String);

#[nutype(
    validate(not_empty),
    derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, AsRef)
)]
pub struct IssueIdentifier(String);

#[nutype(
    validate(not_empty),
    derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, AsRef)
)]
pub struct IssueTitle(String);

#[nutype(
    validate(not_empty),
    derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, AsRef)
)]
pub struct IssueStateName(String);

#[nutype(
    validate(not_empty),
    derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, AsRef)
)]
pub struct NormalizedState(String);

#[nutype(
    validate(regex = "^[a-z0-9._-]+$", not_empty),
    derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, AsRef)
)]
pub struct Label(String);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockerRef {
    pub id: Option<IssueId>,
    pub identifier: Option<IssueIdentifier>,
    pub state: Option<IssueStateName>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Issue {
    pub id: IssueId,
    pub identifier: IssueIdentifier,
    pub title: IssueTitle,
    pub description: Option<String>,
    pub priority: Option<i32>,
    pub state: IssueStateName,
    pub branch_name: Option<String>,
    pub url: Option<String>,
    pub labels: Vec<Label>,
    pub blocked_by: Vec<BlockerRef>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl IssueId {
    pub fn value(&self) -> &str {
        self.as_ref()
    }
}

impl IssueIdentifier {
    pub fn value(&self) -> &str {
        self.as_ref()
    }
}

impl IssueTitle {
    pub fn value(&self) -> &str {
        self.as_ref()
    }
}

impl IssueStateName {
    pub fn value(&self) -> &str {
        self.as_ref()
    }
}

impl NormalizedState {
    pub fn value(&self) -> &str {
        self.as_ref()
    }
}

impl Label {
    pub fn value(&self) -> &str {
        self.as_ref()
    }
}

impl Issue {
    pub fn normalized_state(&self) -> Result<NormalizedState, DomainError> {
        parse_normalized_state(self.state.value())
    }

    pub fn has_required_dispatch_fields(&self) -> bool {
        !(self.id.value().trim().is_empty()
            || self.identifier.value().trim().is_empty()
            || self.title.value().trim().is_empty()
            || self.state.value().trim().is_empty())
    }
}

pub fn parse_issue_id(raw: &str) -> Result<IssueId, DomainError> {
    IssueId::try_new(raw.trim().to_string())
        .map_err(|err| validation_to_domain_error("issue.id", err))
}

pub fn parse_issue_identifier(raw: &str) -> Result<IssueIdentifier, DomainError> {
    IssueIdentifier::try_new(raw.trim().to_string())
        .map_err(|err| validation_to_domain_error("issue.identifier", err))
}

pub fn parse_issue_title(raw: &str) -> Result<IssueTitle, DomainError> {
    IssueTitle::try_new(raw.trim().to_string())
        .map_err(|err| validation_to_domain_error("issue.title", err))
}

pub fn parse_issue_state(raw: &str) -> Result<IssueStateName, DomainError> {
    IssueStateName::try_new(raw.trim().to_string())
        .map_err(|err| validation_to_domain_error("issue.state", err))
}

pub fn parse_normalized_state(raw: &str) -> Result<NormalizedState, DomainError> {
    let normalized = normalize_state_name(raw);
    NormalizedState::try_new(normalized)
        .map_err(|err| validation_to_domain_error("normalized_state", err))
}

pub fn parse_label(raw: &str) -> Result<Label, DomainError> {
    let normalized = normalize_label(raw);
    Label::try_new(normalized).map_err(|err| validation_to_domain_error("issue.label", err))
}
