use thiserror::Error;
use crate::misc::errors::transfer::TransferError as TError;

#[derive(Error, Debug)]
pub enum CommonError {
    #[error("Unsupported protocol: {0}")]
    UnsupportedProtocol(String),

    #[error("TransferError: {0}")]
    TransferError(#[from] TError),

    #[error("NotifyError: {0}")]
    NotifyError(#[from] notify::Error),

    #[error("GrassError: {0}")]
    GrassError(#[from] grass::Error),

    #[error("ConfigNotFound: {0}")]
    ConfigNotFound(String),

    #[error("Error: {0}")]
    Error(String)
}