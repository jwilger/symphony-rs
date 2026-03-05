use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Clone, Parser)]
#[command(name = "symphony", version, about = "Symphony issue orchestration service")]
pub struct CliArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    pub workflow_path: Option<PathBuf>,

    #[arg(long, value_name = "PORT")]
    pub port: Option<u16>,

    #[arg(long)]
    pub once: bool,
}
