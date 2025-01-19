use crate::misc::config;
use crate::misc::errors::common::CommonError;
use crate::misc::util::color_log;
use crate::tasks::{build, deploy, watch};
use async_recursion::async_recursion;
use clap::Parser;
use colored::Color;
use std::env;
use std::error::Error;
use std::path::PathBuf;
use tokio;

pub mod contexts;
pub mod misc;
pub(crate) mod tasks;

#[derive(Parser)]
#[command(author, version, about)]
struct Args {
    #[arg(long)]
    task: String,

    #[arg(long = "working-dir", aliases = ["wd"], value_name = "PATH")]
    working_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    if let Some(working_dir) = &args.working_dir {
        // Convert to absolute path and canonicalize
        let abs_path = if working_dir.is_relative() {
            env::current_dir()?.join(working_dir)
        } else {
            working_dir.clone()
        };

        // Canonicalize to resolve any symlinks and normalize path
        let canonical_path = abs_path.canonicalize().map_err(|e| {
            format!(
                "Failed to resolve working directory path {}: {}",
                abs_path.display(),
                e
            )
        })?;

        env::set_current_dir(&canonical_path).map_err(|e| {
            format!(
                "Failed to change working directory to {}: {}",
                canonical_path.display(),
                e
            )
        })?;
    }

    config::Config::load().await?;

    eprintln!("Starting task: {}", &args.task);
    if let Err(e) = run_task(&args.task).await {
        eprintln!("{}", e);
        std::process::exit(1);
    }
    Ok(())
}

#[async_recursion(?Send)]
async fn run_task(task_name: &str) -> Result<(), CommonError> {
    match task_name {
        "build" => {
            color_log(Color::BrightMagenta, "Building...");
            build::execute().await?;
        }
        "deploy" => {
            run_task("build").await?;
            color_log(Color::BrightMagenta, "Deploying..");
            deploy::execute().await?;
        }
        "watch" => {
            color_log(Color::BrightMagenta, "Watching for changes..");
            watch::execute().await?;
        }
        _ => {
            eprintln!("Unknown task: {}", task_name);
            std::process::exit(1);
        }
    }
    Ok(())
}
