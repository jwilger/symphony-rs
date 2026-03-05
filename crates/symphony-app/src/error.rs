use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("missing_workflow_file")]
    MissingWorkflowFile,
    #[error("orchestrator_error: {0}")]
    OrchestratorError(String),
    #[error("linear_api_request: {0}")]
    LinearApiRequest(String),
    #[error("linear_api_status: {0}")]
    LinearApiStatus(String),
    #[error("linear_graphql_errors: {0}")]
    LinearGraphqlErrors(String),
    #[error("codex_error: {0}")]
    CodexError(String),
    #[error("workspace_error: {0}")]
    WorkspaceError(String),
}
