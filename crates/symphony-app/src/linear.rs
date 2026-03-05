use chrono::{DateTime, Utc};
use reqwest::Client;
use serde_json::{json, Value};

use crate::error::AppError;
use symphony_domain::{
    parse_issue_id, parse_issue_identifier, parse_issue_state, parse_issue_title, parse_label,
    BlockerRef, Issue, TrackerConfig,
};

const PAGE_SIZE: i64 = 50;

#[derive(Clone)]
pub struct LinearClient {
    endpoint: String,
    api_key: String,
    project_slug: String,
    http_client: Client,
}

impl LinearClient {
    pub fn new(config: &TrackerConfig) -> Result<Self, AppError> {
        let http_client = Client::builder()
            .timeout(std::time::Duration::from_millis(30_000))
            .build()
            .map_err(|err| AppError::LinearApiRequest(err.to_string()))?;

        Ok(Self {
            endpoint: config.endpoint.as_ref().to_string(),
            api_key: config.api_key.value().to_string(),
            project_slug: config.project_slug.value().to_string(),
            http_client,
        })
    }

    pub async fn fetch_candidate_issues(&self, active_states: &[String]) -> Result<Vec<Issue>, AppError> {
        self.fetch_issues_by_state_names(active_states).await
    }

    pub async fn fetch_issues_by_states(&self, states: &[String]) -> Result<Vec<Issue>, AppError> {
        if states.is_empty() {
            return Ok(Vec::new());
        }
        self.fetch_issues_by_state_names(states).await
    }

    pub async fn fetch_issue_states_by_ids(&self, issue_ids: &[String]) -> Result<Vec<Issue>, AppError> {
        if issue_ids.is_empty() {
            return Ok(Vec::new());
        }

        let query = r#"
query IssueStatesByIds($ids: [ID!]) {
  issues(filter: { id: { in: $ids } }) {
    nodes {
      id
      identifier
      title
      state { name }
      priority
      createdAt
      updatedAt
    }
  }
}
"#;

        let payload = self
            .graphql(
                query,
                json!({
                    "ids": issue_ids,
                }),
            )
            .await?;

        parse_issue_nodes(&payload)
    }

    pub async fn execute_raw_graphql(
        &self,
        query: &str,
        variables: Option<serde_json::Map<String, Value>>,
    ) -> Result<Value, AppError> {
        let mut body = serde_json::Map::new();
        body.insert("query".to_string(), Value::String(query.to_string()));
        if let Some(variables) = variables {
            body.insert("variables".to_string(), Value::Object(variables));
        }

        let response = self
            .http_client
            .post(&self.endpoint)
            .header("Authorization", self.api_key.clone())
            .json(&Value::Object(body))
            .send()
            .await
            .map_err(|err| AppError::LinearApiRequest(err.to_string()))?;

        if !response.status().is_success() {
            return Err(AppError::LinearApiStatus(format!(
                "status={} reason={}",
                response.status(),
                response
                    .text()
                    .await
                    .unwrap_or_else(|_| "<unable to read body>".to_string())
            )));
        }

        let payload = response
            .json::<Value>()
            .await
            .map_err(|err| AppError::LinearApiRequest(err.to_string()))?;

        if let Some(errors) = payload.get("errors") {
            return Err(AppError::LinearGraphqlErrors(errors.to_string()));
        }

        Ok(payload)
    }

    async fn fetch_issues_by_state_names(&self, states: &[String]) -> Result<Vec<Issue>, AppError> {
        let query = r#"
query CandidateIssues($projectSlug: String!, $stateNames: [String!], $after: String, $first: Int!) {
  issues(
    first: $first,
    after: $after,
    filter: {
      project: { slugId: { eq: $projectSlug } }
      state: { name: { in: $stateNames } }
    }
  ) {
    pageInfo {
      hasNextPage
      endCursor
    }
    nodes {
      id
      identifier
      title
      description
      priority
      branchName
      url
      createdAt
      updatedAt
      state {
        name
      }
      labels {
        nodes {
          name
        }
      }
      inverseRelations {
        nodes {
          relatedIssue {
            id
            identifier
            state {
              name
            }
          }
        }
      }
    }
  }
}
"#;

        let mut after: Option<String> = None;
        let mut accumulated = Vec::new();

        loop {
            let payload = self
                .graphql(
                    query,
                    json!({
                        "projectSlug": self.project_slug,
                        "stateNames": states,
                        "first": PAGE_SIZE,
                        "after": after,
                    }),
                )
                .await?;

            let mut page_issues = parse_issue_nodes(&payload)?;
            accumulated.append(&mut page_issues);

            let page_info = payload
                .pointer("/data/issues/pageInfo")
                .ok_or_else(|| AppError::LinearApiRequest("missing pageInfo".to_string()))?;
            let has_next_page = page_info
                .get("hasNextPage")
                .and_then(Value::as_bool)
                .unwrap_or(false);

            if !has_next_page {
                break;
            }

            after = page_info
                .get("endCursor")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);

            if after.is_none() {
                return Err(AppError::LinearApiRequest(
                    "linear_missing_end_cursor".to_string(),
                ));
            }
        }

        Ok(accumulated)
    }

    async fn graphql(&self, query: &str, variables: Value) -> Result<Value, AppError> {
        self.execute_raw_graphql(
            query,
            variables.as_object().cloned(),
        )
        .await
    }
}

fn parse_issue_nodes(payload: &Value) -> Result<Vec<Issue>, AppError> {
    let nodes = payload
        .pointer("/data/issues/nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::LinearApiRequest("linear_unknown_payload".to_string()))?;

    nodes.iter().map(parse_issue).collect()
}

fn parse_issue(node: &Value) -> Result<Issue, AppError> {
    let id = node
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::LinearApiRequest("missing issue.id".to_string()))?;
    let identifier = node
        .get("identifier")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::LinearApiRequest("missing issue.identifier".to_string()))?;
    let title = node
        .get("title")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::LinearApiRequest("missing issue.title".to_string()))?;
    let state_name = node
        .pointer("/state/name")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::LinearApiRequest("missing issue.state".to_string()))?;

    let labels = node
        .pointer("/labels/nodes")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.get("name").and_then(Value::as_str))
                .filter_map(|name| parse_label(name).ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let blocked_by = node
        .pointer("/inverseRelations/nodes")
        .and_then(Value::as_array)
        .map(|relations| {
            relations
                .iter()
                .map(|relation| {
                    let blocker = relation.get("relatedIssue").cloned().unwrap_or(Value::Null);
                    BlockerRef {
                        id: blocker
                            .get("id")
                            .and_then(Value::as_str)
                            .and_then(|raw| parse_issue_id(raw).ok()),
                        identifier: blocker
                            .get("identifier")
                            .and_then(Value::as_str)
                            .and_then(|raw| parse_issue_identifier(raw).ok()),
                        state: blocker
                            .pointer("/state/name")
                            .and_then(Value::as_str)
                            .and_then(|raw| parse_issue_state(raw).ok()),
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(Issue {
        id: parse_issue_id(id).map_err(|err| AppError::LinearApiRequest(err.to_string()))?,
        identifier: parse_issue_identifier(identifier)
            .map_err(|err| AppError::LinearApiRequest(err.to_string()))?,
        title: parse_issue_title(title).map_err(|err| AppError::LinearApiRequest(err.to_string()))?,
        description: node
            .get("description")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        priority: node
            .get("priority")
            .and_then(Value::as_i64)
            .and_then(|value| i32::try_from(value).ok()),
        state: parse_issue_state(state_name)
            .map_err(|err| AppError::LinearApiRequest(err.to_string()))?,
        branch_name: node
            .get("branchName")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        url: node
            .get("url")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        labels,
        blocked_by,
        created_at: node
            .get("createdAt")
            .and_then(Value::as_str)
            .and_then(parse_timestamp),
        updated_at: node
            .get("updatedAt")
            .and_then(Value::as_str)
            .and_then(parse_timestamp),
    })
}

fn parse_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .map(|parsed| parsed.with_timezone(&Utc))
        .ok()
}
