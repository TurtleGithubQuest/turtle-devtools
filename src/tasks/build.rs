use std::path::{Path, PathBuf};
use tokio::process::Command;
use colored::Color;
use crate::contexts;
use crate::misc::config::CONFIG;
use crate::misc::errors::common::CommonError;
use crate::misc::errors::transfer;
use crate::misc::util::{color_log};

async fn run_command(
    command: &str,
    args: &[&str],
    cwd: Option<&Path>,
    is_quiet: bool,
) -> Result<(), CommonError> {
    let mut cmd = Command::new(command);
    cmd.args(args);

    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }

    if is_quiet {
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());
    } else {
        cmd.stdout(std::process::Stdio::inherit());
        cmd.stderr(std::process::Stdio::inherit());
    }

    let status = cmd.status().await.map_err(transfer::TransferError::IoError)?;

    if !status.success() {
        let code = status.code().unwrap_or(-1);
        return Err(CommonError::Error(format!(
            "Command `{}` with args {:?} exited with code {}",
            command, args, code
        )));
    }

    Ok(())
}

async fn build_composer() -> Result<(), CommonError> {
    let composer_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("website/composer");

    // Check if composer directory exists
    if !composer_dir.exists() {
        return Ok(());
    }

    color_log(Color::Yellow, "Building Composer...");

    // Try to run 'composer --version' in composer_dir
    match run_command("composer", &["--version"], Some(&composer_dir), false).await {
        Ok(_) => {
            // Then run 'composer install --no-dev --optimize-autoloader' in composer_dir
            if let Err(error) = run_command(
                "composer",
                &["install", "--no-dev", "--optimize-autoloader"],
                Some(&composer_dir),
                false,
            ).await {
                color_log(
                    Color::Red,
                    &format!(
                        "Composer is not available or encountered an error:\n{}",
                        error
                    ),
                );
                color_log(
                    Color::Yellow,
                    "Please make sure Composer is installed and accessible from the command line.",
                );
                color_log(
                    Color::Yellow,
                    "You can download Composer from https://getcomposer.org/",
                );
                return Err(CommonError::Error("Composer build failed. See above for details.".into()));
            } else {
                color_log(Color::Yellow, "Composer build completed.");
            }
        }
        Err(error) => {
            color_log(
                Color::Red,
                &format!("Error checking Composer version: {}", error),
            );
            return Err(CommonError::Error("Failed to check Composer version.".into()));
        }
    }

    Ok(())
}

async fn build_javascript(is_quiet: bool) -> Result<(), Box<dyn std::error::Error>> {
    let package_json = Path::new("package.json");

    if !package_json.exists() {
        return Ok(());
    }

    (!is_quiet).then(|| color_log(Color::Yellow, "Building JavaScript..."));

    // Determine which package manager to use
    let use_bun = Path::new("bun.lockb").exists();
    let use_yarn = Path::new("yarn.lock").exists();
    let use_npm = Path::new("package-lock.json").exists() || (!use_bun && !use_yarn);

    if use_bun {
        run_command("bun", &["install"], None, is_quiet).await?;
        run_command("bun", &["run", "build"], None, is_quiet).await?;
    } else if use_yarn {
        run_command("yarn", &["install"], None, is_quiet).await?;
        run_command("yarn", &["run", "build"], None, is_quiet).await?;
    } else if use_npm {
        run_command("npm", &["install"], None, is_quiet).await?;
        run_command("npm", &["run", "build"], None, is_quiet).await?;
    } else {
        return Err("No recognized package manager lock file found. Please ensure you have either bun.lockb, package-lock.json, or yarn.lock.".into());
    }

    (!is_quiet).then(|| color_log(Color::Yellow, "JavaScript build completed."));

    Ok(())
}

pub async fn execute() -> Result<(), CommonError> {
    if let Err(error) = build_composer().await {
        color_log(Color::Red, &format!("Composer build failed:\n{}", error));
        std::process::exit(1);
    }

    if let Err(error) = build_javascript(false).await {
        color_log(Color::Red, &format!("JavaScript build failed:\n{}", error));
        std::process::exit(1);
    }
    
    let config = CONFIG.get().ok_or_else(|| CommonError::ConfigNotFound("Config not loaded".into()))?;
    let mut contexts: Vec<Box<dyn contexts::BuildContext>> = Vec::new();

    if let Some(scss_context) = &config.contexts.scss {
        contexts.push(Box::new(scss_context.clone()));
    }
    
    for context in contexts {
        context.build(None)?;
    }
    
    color_log(Color::Green, "Build process completed successfully.");
    Ok(())
}