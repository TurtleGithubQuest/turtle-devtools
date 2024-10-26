use std::error::Error;
use std::path::Path;
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

    fn build(&self, changed_file: Option<&Path>) -> Result<(), CommonError> {
        self.log();
        let output_folder = self.get_output_folder()?;

        if !output_folder.exists() {
            std::fs::create_dir_all(&output_folder).map_err(transfer::TransferError::IoError)?;
        }
        
        if let Some(changed_file) = changed_file {
            self.build(changed_file, &output_folder)?;
        } else {
            for entrypoint in self.get_entrypoints() {
                let entrypoint_path = Path::new(&entrypoint.entrypoint);
                self.build(entrypoint_path, &output_folder)?;
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

        let css = grass::from_path(
            scss_path.to_str().unwrap(),
            &grass::Options::default(),
        ).unwrap();

        let file_stem = scss_path.file_stem().unwrap();
        let css_output_path = output_folder.join(format!("{}.css", file_stem.to_string_lossy()));

        std::fs::write(&css_output_path, css.as_bytes()).map_err(TransferError::IoError)?;

        println!("Compiled {:?} to {:?}", scss_path, css_output_path);

        Ok(())
    }
}