use thiserror::Error;

#[derive(Error, Debug)]
pub enum TransferError {
    #[error("Unsupported protocol: {0}")]
    UnsupportedProtocol(String),

    #[error("SSH error: {0}")]
    SshError(#[from] ssh2::Error),

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("FTP error: {0}")]
    FtpError(#[from] async_ftp::FtpError),

    #[error("Missing environment variables: {0:?}")]
    MissingEnvironmentVariables(Vec<String>),

    #[error("Environment variable '{0}' is invalid (contains non-unicode data).")]
    InvalidEnvironmentVariable(String),

    #[error("Tokio task join error: {0}")]
    JoinError(#[from] tokio::task::JoinError),

    #[error("Authentication failed")]
    AuthenticationFailed,

    #[error("Path error: {0}")]
    PathError(String),
    
    #[error("Invalid port number: {0}")]
    InvalidPort(String),
}