use crate::contexts::BuildContext;
use crate::misc::config::CONFIG;
use crate::misc::errors::common::CommonError;
use crate::misc::errors::transfer::TransferError;
use crate::misc::transfer::{get_transfer, upload_directory, FileTransfer};
use crate::misc::util::{color_log, get_credentials};
use colored::Color;
use notify::event::{DataChange, ModifyKind};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use swc::{
    config::{Config, JscConfig, ModuleConfig, Options},
    Compiler,
};
use swc_common::{errors::Handler, FileName, SourceMap};
use swc_ecma_parser::{Syntax, TsConfig};
use tokio::select;
use tokio::sync::mpsc;
use tokio::task::LocalSet;

pub async fn execute() -> Result<(), CommonError> {
    let local_set = LocalSet::new();
    let (tx, mut rx) = mpsc::channel(1000);

    let mut watcher = RecommendedWatcher::new(
        move |res| {
            let tx = tx.clone();
            if let Err(e) = tx.try_send(res) {
                color_log(
                    Color::Red,
                    format!("Failed to send file change notification: {:?}", e),
                );
            }
        },
        notify::Config::default(),
    )?;

    let config = CONFIG
        .get()
        .ok_or_else(|| CommonError::ConfigNotFound("Config not loaded".into()))?;
    let mut paths_to_watch = Vec::new();

    let result: Result<(), CommonError> = local_set.run_until(async move {
        if let Some(ref scss_context) = config.contexts.scss {
            for entry in scss_context.get_entrypoints() {
                //paths_to_watch.push(PathBuf::from(&entry.folder));
            }
        }

        if let Some(ref js_context) = config.contexts.js {
            for entry in js_context.get_entrypoints() {
                paths_to_watch.push(PathBuf::from(&entry.folder));
            }
        }

        for path in &paths_to_watch {
            watcher.watch(path, RecursiveMode::Recursive).map_err(|_| TransferError::PathError( format!("Failed to watch {}", path.to_str().unwrap_or("<unknown>")) ) )?;
            color_log(Color::Green, &format!("Watching {:?}", path));
        }

        // Load credentials
        dotenv::dotenv().ok();
        let protocol = std::env::var("PROTOCOL").unwrap_or_else(|_| "ssh".to_string());
        let credentials = get_credentials(&protocol)?;

        let mut transfer = get_transfer(&protocol, &credentials).await?;

        color_log(Color::Green, "Started watching for file changes...");

        let mut debounce_map: HashMap<PathBuf, Instant> = HashMap::new();
        let debounce_duration = Duration::from_millis(500);

        let mut interval = tokio::time::interval(Duration::from_millis(100));

        // Process events
        loop {
            select! {
                Some(Ok(event)) = rx.recv() => {
                    if let Some(changed_path) = event.paths.first() {
                        let file_name = changed_path.file_name().unwrap_or_default();

                        // Skip temporary or backup files (e.g., files ending with '~')
                        if file_name.to_string_lossy().ends_with('~') {
                            continue;
                        }
                        match event.kind {
                            EventKind::Modify(ModifyKind::Data(DataChange::Any)) |
                            EventKind::Create(_) => {
                                if changed_path.is_file() {
                                    // Check if it's a JS/TS file
                                    if let Some(ext) = changed_path.extension() {
                                        if ext == "js" || ext == "ts" || ext == "tsx" {
                                            debounce_map.insert(changed_path.clone(), Instant::now());
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ = interval.tick() => {
                    // Check debounce_map for paths to process
                    let now = Instant::now();
                    let mut to_process = Vec::new();

                    debounce_map.retain(|path, &mut last_change| {
                        if now.duration_since(last_change) >= debounce_duration {
                            // Time to process this path
                            to_process.push(path.clone());
                            false // Remove from the map
                        } else {
                            true // Retain in the map
                        }
                    });

                    for path in to_process {
                        if let Err(e) = handle_file_change(&mut transfer, &path, config.clone()).await {
                            color_log(Color::Red, &format!("Error handling file change: {:?}", e));
                        }
                    }
                }
                else => {
                    // Channel closed or error occurred, exit loop
                    break;
                }
            }
        }

        // Disconnect the transfer (though this will likely never be reached)
        transfer.disconnect().await?;
        Ok(())
    }).await;

    result
}

async fn handle_file_change(
    transfer: &mut Box<dyn FileTransfer>,
    changed_path: &PathBuf,
    config: crate::config::Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let absolute_file_path = changed_path
        .canonicalize()
        .unwrap_or_else(|_| changed_path.to_path_buf());
    color_log(
        Color::BrightCyan,
        &format!("Changed file: {:?}", absolute_file_path),
    );

    let mut contexts_to_build: Vec<&dyn BuildContext> = Vec::new();

    if let Some(ref scss_context) = config.contexts.scss {
        if scss_context.is_file_in_context(&absolute_file_path) {
            contexts_to_build.push(scss_context);
        }
    }

    if let Some(ref js_context) = config.contexts.js {
        if js_context.is_file_in_context(&absolute_file_path) {
            contexts_to_build.push(js_context);
        }
    }

    let remote_base_path = std::env::var("REMOTE_PATH")
        .map_err(|_| CommonError::Error("REMOTE_PATH must be set in .env".to_string()))?;

    for context in contexts_to_build {
        color_log(
            Color::Yellow,
            &format!("Building {} context...", context.context_name()),
        );
        context.build(Some(&absolute_file_path))?;

        let output_folder = context.get_output_folder()?;

        // Construct the full remote path by joining the base path with the context name
        let remote_path = PathBuf::from(&remote_base_path)
            .join(context.context_name())
            .to_string_lossy()
            .to_string();

        color_log(
            Color::Cyan,
            &format!("Uploading to remote path: {}", remote_path),
        );

        upload_directory(&mut *transfer, &output_folder, &remote_path).await?;
    }

    Ok(())
}
