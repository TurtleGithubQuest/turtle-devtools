use crate::misc::config::CONFIG;
use crate::misc::errors::common::CommonError;
use crate::misc::transfer;

pub async fn execute() -> Result<(), CommonError> {
    let config = CONFIG.get().ok_or_else(|| CommonError::ConfigNotFound("Config not loaded".into()))?;
    let output_folder = config.output_folder.as_ref();

    transfer::transfer_build_folder(output_folder).await?;

    Ok(())
}