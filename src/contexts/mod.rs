use std::error::Error;
use std::path::{Path, PathBuf};
use colored::Color;
use serde::Deserialize;
use crate::config::CONFIG;
use crate::misc::errors::common::CommonError;
use crate::misc::util::color_log;

pub mod scss;
pub mod js;

pub trait BuildContext {
    fn context_name(&self) -> &str;

    fn base(&self) -> &BaseContext;
    
    fn log(&self) {
        color_log(
            Color::Yellow, 
            &format!("Building {}", self.context_name())
        );
    }

    fn build(&self, path: Option<&PathBuf>) -> Result<(), CommonError>;

    fn get_output_folder(&self) -> Result<PathBuf, CommonError> {
        let config = CONFIG.get().ok_or_else(|| CommonError::ConfigNotFound("Config not loaded".into()))?;
        let global_output_folder = config.output_folder.as_str();

        let context_output_folder = self.base().output_folder.as_deref().unwrap_or(self.context_name());
        let output_folder = Path::new(global_output_folder).join(context_output_folder);

        Ok(output_folder)
    }

    fn get_entrypoints(&self) -> &Vec<EntryPoint> {
        &self.base().entrypoints
    }
    fn is_file_in_context(&self, path: &Path) -> bool {
        let absolute_file_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    
        self.get_entrypoints().iter().any(|entry| {
            let folder_path = Path::new(&entry.folder);
            let absolute_folder_path = folder_path.canonicalize().unwrap_or_else(|_| {
                std::env::current_dir().unwrap().join(folder_path)
            });
    
            absolute_file_path.starts_with(&absolute_folder_path)
        })
    }
}

#[derive(Deserialize, Clone)]
pub struct BaseContext {
    pub output_folder: Option<String>,
    pub entrypoints: Vec<EntryPoint>,
}

#[derive(Deserialize, Clone)]
pub struct EntryPoint {
    pub folder: String,
    pub entrypoint: String,
}