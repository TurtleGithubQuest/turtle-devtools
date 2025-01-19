use crate::contexts::{BaseContext, BuildContext, EntryPoint};
use crate::misc::config::CONFIG;
use crate::misc::errors::common::CommonError;
use crate::misc::util::color_log;
use anyhow::{Context as AnyhowContext, Result};
use colored::Color;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use swc::{
    config::{Config, JscConfig, ModuleConfig, Options},
    Compiler, PrintArgs,
};
use swc_common::{errors::Handler, FileName, SourceMap};
use swc_ecma_parser::{Syntax, TsConfig};

#[derive(Deserialize, Clone)]
pub struct Context {
    #[serde(flatten)]
    base: BaseContext,
}

impl BuildContext for Context {
    fn context_name(&self) -> &str {
        "js"
    }

    fn base(&self) -> &BaseContext {
        &self.base
    }

    fn build(&self, path: Option<&PathBuf>) -> Result<(), CommonError> {
        let compiler = Compiler::new(Arc::new(SourceMap::default()));

        // If a specific path is provided, build only that file
        if let Some(file_path) = path {
            self.compile_file(&compiler, file_path)?;
        } else {
            // Otherwise, build all files in the context
            for entry in &self.base.entrypoints {
                let entry_folder = PathBuf::from(&entry.folder);
                self.compile_folder(&compiler, &entry_folder)?;
            }
        }

        Ok(())
    }

    fn get_output_folder(&self) -> Result<PathBuf, CommonError> {
        let config = CONFIG
            .get()
            .ok_or_else(|| CommonError::ConfigNotFound("Config not loaded".into()))?;
        let global_output_folder = PathBuf::from(&config.output_folder);

        let context_output_folder = self
            .base()
            .output_folder
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("js")); // Default to "js" if not specified

        // Combine global output folder with context-specific folder
        Ok(global_output_folder.join(context_output_folder))
    }

    fn get_entrypoints(&self) -> &Vec<EntryPoint> {
        &self.base.entrypoints
    }
}

impl Context {
    fn compile_file(&self, compiler: &Compiler, file_path: &PathBuf) -> Result<(), CommonError> {
        // Load the file into the source map
        let fm = compiler
            .cm
            .load_file(file_path)
            .map_err(|e| CommonError::Error(format!("Failed to load file: {:?}", e)))?;

        // Configure SWC options
        let options = Options {
            config: Config {
                jsc: JscConfig {
                    syntax: Some(Syntax::Typescript(TsConfig::default())),
                    ..Default::default()
                },
                module: Some(ModuleConfig::CommonJs(Default::default())), // Output as CommonJS
                ..Default::default()
            },
            output_path: Some(self.get_output_path(file_path)?),
            ..Default::default()
        };

        // Create a handler for error reporting
        let handler =
            Handler::with_emitter_writer(Box::new(std::io::stderr()), Some(compiler.cm.clone()));

        // Parse and transform the TypeScript file
        let program = compiler
            .parse_js(
                fm,
                &handler,
                swc_ecma_ast::EsVersion::Es2022,
                Syntax::Typescript(TsConfig::default()),
                swc::config::IsModule::Bool(true),
                Some(compiler.comments()),
            )
            .map_err(|e| CommonError::Error(format!("Failed to parse TypeScript: {:?}", e)))?;

        // Print the transformed JavaScript code
        let output = compiler
            .print(&program, PrintArgs::default())
            .map_err(|e| CommonError::Error(format!("Failed to print JavaScript: {:?}", e)))?;

        // Write the output to the destination file
        let output_path = self.get_output_path(file_path)?;
        std::fs::write(&output_path, output.code)
            .map_err(|e| CommonError::Error(format!("Failed to write output file: {:?}", e)))?;

        Ok(())
    }

    fn compile_folder(
        &self,
        compiler: &Compiler,
        folder_path: &PathBuf,
    ) -> Result<(), CommonError> {
        // Walk through the folder and compile all TypeScript files
        for entry in ignore::Walk::new(folder_path)
            .filter_map(Result::ok)
            .filter(|e| {
                e.path()
                    .extension()
                    .map_or(false, |ext| ext == "ts" || ext == "tsx")
            })
        {
            self.compile_file(compiler, &entry.path().to_path_buf())?;
        }

        Ok(())
    }

    fn get_output_path(&self, input_path: &PathBuf) -> Result<PathBuf, CommonError> {
        for entrypoint in &self.base.entrypoints {
            let base_path = PathBuf::from(&entrypoint.folder);

            // Get absolute paths for both the entrypoint and input file
            let absolute_base = base_path
                .canonicalize()
                .unwrap_or_else(|_| base_path.clone());
            let absolute_input = input_path
                .canonicalize()
                .unwrap_or_else(|_| input_path.clone());

            // Try to compute relative path for each entrypoint
            if let Ok(relative_path) = absolute_input.strip_prefix(&absolute_base) {
                // Construct the output path by joining the build folder with the relative path
                let output_path = self
                    .get_output_folder()?
                    .join(relative_path)
                    .with_extension("js");

                // Ensure the output directory exists
                if let Some(parent) = output_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        CommonError::Error(format!("Failed to create output directory: {:?}", e))
                    })?;
                }

                return Ok(output_path);
            }
        }

        Err(CommonError::Error(format!(
            "File {:?} is not within any entrypoint folder. Entrypoints: {:?}",
            input_path,
            self.base
                .entrypoints
                .iter()
                .map(|e| &e.folder)
                .collect::<Vec<_>>()
        )))
    }
}
