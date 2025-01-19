use std::path::{Path, PathBuf};
use grass;
use serde::Deserialize;
use crate::contexts::{BuildContext, BaseContext};
use crate::misc::errors::common::CommonError;
use crate::misc::errors::transfer;
use crate::misc::errors::transfer::TransferError;

#[derive(Deserialize, Clone)]
pub struct Context {
    #[serde(flatten)]
    base: BaseContext,
}

impl BuildContext for Context {
    fn context_name(&self) -> &str {
        "scss"
    }

    fn base(&self) -> &BaseContext {
        &self.base
    }

    fn build(&self, path: Option<&PathBuf>) -> Result<(), CommonError> {
        self.log();
        let output_folder = self.get_output_folder()?;

        if !output_folder.exists() {
            std::fs::create_dir_all(&output_folder).map_err(transfer::TransferError::IoError)?;
        }
        
        if let Some(path) = path {
            self.build(path.as_path(), &output_folder)?;
        } else {
            for entrypoint in self.get_entrypoints() {
                if let Some(entrypoint_path) = &entrypoint.entrypoint {
                    let entrypoint_path = Path::new(entrypoint_path);
                    self.build(entrypoint_path, &output_folder)?;
                }
            }
        }
        Ok(())
    }
}

impl Context {
    pub fn build(&self, scss_path: &Path, output_folder: &Path) -> Result<(), CommonError> {
        if !scss_path.exists() {
            return Err(TransferError::PathError(format!("SCSS file {:?} does not exist.", scss_path)).into());
        }
        
        if scss_path.is_dir() {
            return Err(CommonError::Error(format!("Scss path is a folder {:?}.", scss_path)));
        }
        
        let css = grass::from_path(
            scss_path.to_str().unwrap(),
            &grass::Options::default(),
        ).map_err(|err| CommonError::GrassError(*err))?;

        let file_stem = scss_path.file_stem().unwrap();
        let css_output_path = output_folder.join(format!("{}.css", file_stem.to_string_lossy()));

        std::fs::write(&css_output_path, css.as_bytes()).map_err(TransferError::IoError)?;

        println!("Compiled {:?} to {:?}", scss_path, css_output_path);

        Ok(())
    }
}