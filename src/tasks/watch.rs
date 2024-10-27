use std::ops::Deref;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use std::path::{Path, PathBuf};
use crate::misc::util::{color_log, get_credentials};
use tokio::task::LocalSet;
use crate::misc::transfer::{FileTransfer, upload_directory, get_transfer};
use colored::Color;
use notify::event::{DataChange, ModifyKind};
use crate::contexts::BuildContext;
use crate::misc::config::{CONFIG};
use crate::misc::errors::common::CommonError;
use crate::misc::errors::transfer::TransferError;

pub async fn execute() -> Result<(), CommonError> {
    let local_set = LocalSet::new();
    // Create a channel to receive file system events
    let (tx, mut rx) = mpsc::channel(100);

    // Create a watcher object
    let mut watcher = RecommendedWatcher::new(
        move |res| {
            futures::executor::block_on(async {
                tx.send(res).await.unwrap();
            })
        },
        notify::Config::default(),
    )?;
    
    let config = CONFIG.get().ok_or_else(|| CommonError::ConfigNotFound("Config not loaded".into()))?;
    let mut paths_to_watch = Vec::new();

    let result: Result<(), crate::misc::errors::common::CommonError> = local_set.run_until(async move {
        if let Some(ref scss_context) = config.contexts.scss {
            for entry in scss_context.get_entrypoints() {
                paths_to_watch.push(PathBuf::from(&entry.folder));
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

        // Process events
        while let Some(Ok(event)) = rx.recv().await {
            if event.kind == EventKind::Modify(ModifyKind::Data(DataChange::Content)) {
                continue;
            }
            if let Some(path) = event.paths.first() {
                if let Err(e) = handle_file_change(&mut transfer, path, config.clone()).await {
                    color_log(Color::Red,&format!("Error handling file change: {:?}", e) );
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
    let mut contexts_to_build: Vec<&dyn BuildContext> = Vec::new();

    if let Some(ref scss_context) = config.contexts.scss {
        if scss_context.is_file_in_context(changed_path) {
            contexts_to_build.push(scss_context);
        }
    }

    if let Some(ref js_context) = config.contexts.js {
        if js_context.is_file_in_context(changed_path) {
            contexts_to_build.push(js_context);
        }
    }
    
    for context in contexts_to_build {
        color_log(Color::Yellow, &format!("Building {} context...", context.context_name()));
        context.build(Some(changed_path))?;

        let output_folder = context.get_output_folder()?;

        let remote_path = context.context_name();

        upload_directory(
            &mut *transfer,
            &output_folder,
            remote_path,
        )
        .await?;
    }

    Ok(())
}