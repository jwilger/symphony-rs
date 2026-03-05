use std::fmt::{Display, Formatter};

use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DomainError {
    #[error("invalid value for {field}: {reason}")]
    InvalidValue { field: &'static str, reason: String },
    #[error("missing required field: {0}")]
    MissingField(&'static str),
}

impl DomainError {
    pub fn invalid(field: &'static str, reason: impl Display) -> Self {
        Self::InvalidValue {
            field,
            reason: reason.to_string(),
        }
    }

    pub fn missing(field: &'static str) -> Self {
        Self::MissingField(field)
    }
}

pub fn validation_to_domain_error(
    field: &'static str,
    err: impl Into<Box<dyn std::error::Error + Send + Sync>>,
) -> DomainError {
    let error_box = err.into();
    DomainError::invalid(field, ValidationErrorDisplay(error_box.to_string()))
}

struct ValidationErrorDisplay(String);

impl Display for ValidationErrorDisplay {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
