use std::collections::HashMap;
use std::path::{Path, PathBuf};

use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc::UnboundedSender;

use crate::error::AppError;
use crate::events::ServiceEvent;
use symphony_core::{build_service_config, parse_workflow, ConfigError, WorkflowError};
use symphony_domain::{ServiceConfig, WorkflowDefinition};

#[derive(Debug, Clone)]
pub struct LoadedWorkflow {
    pub workflow_path: PathBuf,
    pub workflow: WorkflowDefinition,
    pub config: ServiceConfig,
}

pub fn discover_workflow_path(explicit_path: Option<PathBuf>) -> Result<PathBuf, AppError> {
    let path = explicit_path.unwrap_or_else(|| PathBuf::from("WORKFLOW.md"));
    if !path.exists() {
        return Err(AppError::MissingWorkflowFile);
    }
    Ok(path)
}

pub fn load_workflow_file(path: &Path) -> Result<LoadedWorkflow, AppError> {
    let contents = std::fs::read_to_string(path).map_err(map_workflow_error)?;
    let workflow = parse_workflow(&contents).map_err(map_workflow_error)?;
    let environment = std::env::vars().collect::<HashMap<_, _>>();
    let config = build_service_config(&workflow, &environment).map_err(map_config_error)?;

    Ok(LoadedWorkflow {
        workflow_path: path.to_path_buf(),
        workflow,
        config,
    })
}

pub fn spawn_workflow_watcher(path: PathBuf, sender: UnboundedSender<ServiceEvent>) -> anyhow::Result<()> {
    let watched_path = path.canonicalize().unwrap_or(path.clone());

    let mut watcher = RecommendedWatcher::new(
        move |event: notify::Result<notify::Event>| {
            let Ok(event) = event else {
                return;
            };

            let workflow_changed = matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
            );

            if !workflow_changed {
                return;
            }

            if let Ok(loaded) = load_workflow_file(&watched_path) {
                let _ = sender.send(ServiceEvent::WorkflowReloaded {
                    workflow: loaded.workflow,
                    config: loaded.config,
                });
            }
        },
        Config::default(),
    )?;

    watcher.watch(&watched_path, RecursiveMode::NonRecursive)?;

    std::thread::Builder::new()
        .name("workflow-watcher-retainer".to_string())
        .spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3600));
                let _ = &watcher;
            }
        })?;

    Ok(())
}

fn map_workflow_error(error: impl std::fmt::Display) -> AppError {
    let rendered = error.to_string();
    if rendered.contains(&WorkflowError::MissingWorkflowFile.to_string()) {
        AppError::MissingWorkflowFile
    } else {
        AppError::OrchestratorError(rendered)
    }
}

fn map_config_error(error: ConfigError) -> AppError {
    AppError::OrchestratorError(error.to_string())
}
