use std::env;
use std::path::PathBuf;
use std::error::Error;
use async_recursion::async_recursion;
use clap::Parser;
use colored::Color;
use tokio;
use crate::misc::util::color_log;
use crate::tasks::{build, deploy, watch};
use crate::misc::config;
use crate::misc::errors::common::CommonError;

pub(crate) mod tasks;
pub mod misc;
pub mod contexts;

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
        if let Err(e) = env::set_current_dir(working_dir) {
            eprintln!(
                "Failed to change working directory to {}: {}", working_dir.display(), e
            );
            std::process::exit(1);
        }
    }

    config::Config::load().await?;

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