use crate::misc::errors::transfer::TransferError;
use crate::misc::util::{color_log, get_credentials};
use anyhow::__private::kind::AdhocKind;
use async_ftp::FtpStream;
use async_recursion::async_recursion;
use async_trait::async_trait;
use colored::Color;
use dotenv::dotenv;
use ssh2::Session;
use std::env;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub(crate) struct Credentials {
    pub host: String,
    pub username: String,
    pub auth: AuthMethod,
    pub port: u16,
    pub remote_dir: String,
}

#[derive(Clone)]
pub enum AuthMethod {
    Password(String),
    Key(String),
}

#[async_trait]
pub trait FileTransfer {
    async fn begin(credentials: &Credentials) -> Result<Self, TransferError>
    where
        Self: Sized;
    async fn change_directory(&mut self, path: &str) -> Result<(), TransferError>;
    async fn create_directory(&mut self, path: &str) -> Result<(), TransferError>;
    async fn upload_file(
        &mut self,
        local_path: &Path,
        remote_filename: &str,
    ) -> Result<(), TransferError>;
    async fn disconnect(&mut self) -> Result<(), TransferError>;
}

#[async_trait]
impl FileTransfer for FtpStream {
    async fn begin(credentials: &Credentials) -> Result<Self, TransferError> {
        let address = format!("{}:{}", credentials.host, credentials.port);
        let mut ftp_stream = FtpStream::connect(address).await?;

        if let AuthMethod::Password(password) = &credentials.auth {
            ftp_stream.login(&credentials.username, password).await?;
        } else {
            return Err(TransferError::AuthenticationFailed);
        }

        // Set transfer type to Binary
        ftp_stream
            .transfer_type(async_ftp::types::FileType::Binary)
            .await?;
        Ok(ftp_stream)
    }

    async fn change_directory(&mut self, path: &str) -> Result<(), TransferError> {
        self.cwd(path).await.map_err(TransferError::from)
    }

    async fn create_directory(&mut self, path: &str) -> Result<(), TransferError> {
        self.mkdir(path).await.map_err(TransferError::from)
    }

    async fn upload_file(
        &mut self,
        local_path: &Path,
        remote_filename: &str,
    ) -> Result<(), TransferError> {
        use tokio::fs::File;
        let mut local_file = File::open(local_path).await?;
        self.put(remote_filename, &mut local_file)
            .await
            .map_err(TransferError::from)
    }

    async fn disconnect(&mut self) -> Result<(), TransferError> {
        self.quit().await.map_err(TransferError::from)
    }
}

pub struct SshFileTransfer {
    session: Session,
}

#[async_trait]
impl FileTransfer for SshFileTransfer {
    async fn begin(credentials: &Credentials) -> Result<Self, TransferError> {
        use std::net::TcpStream;
        use tokio::task;

        let credentials = credentials.clone();
        let session = task::spawn_blocking(move || {
            let address = format!("{}:{}", credentials.host, credentials.port);
            color_log(Color::Green, format!("Connecting to: {address}"));
            let tcp = TcpStream::connect(address)?;
            let mut session = Session::new()?;
            session.set_tcp_stream(tcp);
            session.handshake()?;

            match &credentials.auth {
                AuthMethod::Password(password) => {
                    session.userauth_password(&credentials.username, password)?;
                }
                AuthMethod::Key(key_content) => {
                    session.userauth_pubkey_memory(
                        &credentials.username,
                        None,
                        key_content,
                        None,
                    )?;
                }
            }

            if !session.authenticated() {
                return Err(TransferError::AuthenticationFailed);
            }
            Ok(session)
        })
        .await??;

        Ok(SshFileTransfer { session })
    }

    async fn change_directory(&mut self, _path: &str) -> Result<(), TransferError> {
        // SSH doesn't need to change directories for SFTP
        Ok(())
    }

    async fn create_directory(&mut self, path: &str) -> Result<(), TransferError> {
        use tokio::task;

        let sftp = self.session.sftp()?;
        let path = path.to_string();
        task::spawn_blocking(move || {
            sftp.mkdir(Path::new(&path), 0o755)
                .map_err(TransferError::from)
        })
        .await??;
        Ok(())
    }

    async fn upload_file(
        &mut self,
        local_path: &Path,
        remote_filename: &str,
    ) -> Result<(), TransferError> {
        let session = self.session.clone();
        let local_path = local_path.to_path_buf();
        let remote_filename = remote_filename.to_string();

        tokio::task::spawn_blocking(move || -> Result<(), TransferError> {
            let sftp = session.sftp().map_err(TransferError::SshError)?;

            // Ensure remote directory exists
            if let Some(parent_dir) = Path::new(&remote_filename).parent() {
                let parent_dir_str = parent_dir.to_string_lossy().to_string();

                // Try to create parent directories recursively via SFTP
                let mut current_path = String::new();
                for segment in parent_dir_str.split('/').filter(|s| !s.is_empty()) {
                    if !current_path.is_empty() {
                        current_path.push('/');
                    }
                    current_path.push_str(segment);

                    // Try to create directory if it doesn't exist
                    match sftp.mkdir(Path::new(&current_path), 0o755) {
                        Ok(_) => println!("Created directory: {}", current_path),
                        Err(e) => {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                format!("Failed to create directory {}: {}", current_path, e),
                            )
                            .into());
                        }
                    }
                }
            }

            // Open local file
            let mut local_file =
                std::fs::File::open(&local_path).map_err(TransferError::IoError)?;

            // Create remote file
            let remote_file_path = Path::new(&remote_filename);
            let mut remote_file = sftp.create(remote_file_path).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to create remote file {}: {}", remote_filename, e),
                )
            })?;

            // Copy file contents
            std::io::copy(&mut local_file, &mut remote_file).map_err(TransferError::IoError)?;

            Ok(())
        })
        .await
        .map_err(TransferError::JoinError)??;

        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), TransferError> {
        // No explicit disconnect needed for SSH
        Ok(())
    }
}

pub async fn get_transfer(
    protocol: &str,
    credentials: &Credentials,
) -> Result<Box<dyn FileTransfer>, TransferError> {
    match protocol {
        "ssh" => {
            let transfer = SshFileTransfer::begin(&credentials).await?;
            Ok(Box::new(transfer))
        }
        "ftp" => {
            let transfer = async_ftp::FtpStream::begin(&credentials).await?;
            Ok(Box::new(transfer))
        }
        _ => {
            Err(crate::misc::errors::transfer::TransferError::UnsupportedProtocol(protocol.into()))
        }
    }
}

pub async fn transfer_build_folder(build_folder: &str) -> Result<(), TransferError> {
    dotenv().ok();
    let protocol = env::var("PROTOCOL").unwrap_or_else(|_| "ssh".to_string());
    let remote_path = env::var("REMOTE_PATH").expect("REMOTE_PATH must be set in .env");

    color_log(Color::BrightCyan, &format!("Using protocol: {}", protocol));
    color_log(Color::BrightCyan, &format!("Remote path: {}", remote_path));

    let credentials = get_credentials(&protocol)?;
    color_log(
        Color::BrightCyan,
        &format!("Connecting to: {}:{}", credentials.host, credentials.port),
    );

    let mut transfer = get_transfer(&protocol, &credentials).await?;
    color_log(Color::Green, "Connected successfully");

    // Pass the remote_path from .env to upload_directory
    upload_directory(&mut transfer, Path::new(build_folder), &remote_path).await?;
    transfer.disconnect().await?;

    color_log(Color::Green, "Transfer completed");
    Ok(())
}

#[async_recursion(?Send)]
pub async fn upload_directory(
    transfer: &mut Box<dyn FileTransfer>,
    local_path: &Path,
    remote_base_path: &str,
) -> Result<(), TransferError> {
    // Ensure the remote base path exists
    ensure_remote_directory(transfer, remote_base_path).await?;

    let mut entries = tokio::fs::read_dir(local_path).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let relative_path = path
            .strip_prefix(local_path)
            .map_err(|e| TransferError::PathError(format!("Failed to get relative path: {}", e)))?;

        // Construct the remote equivalent of this local path
        let remote_path = Path::new(remote_base_path).join(relative_path);
        let remote_path_str = remote_path.to_string_lossy().to_string();

        if path.is_dir() {
            // Ensure the remote subdirectory exists
            color_log(
                Color::BrightYellow,
                &format!("📁 Creating directory: {}", remote_path_str),
            );
            ensure_remote_directory(transfer, &remote_path_str).await?;
            upload_directory(transfer, &path, &remote_path_str).await?;
        } else {
            let file_name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            let remote_dir = remote_path
                .parent()
                .ok_or_else(|| TransferError::PathError("Invalid remote path".to_string()))?
                .to_string_lossy()
                .to_string();

            // Ensure the parent directory on the remote side exists
            ensure_remote_directory(transfer, &remote_dir).await?;

            color_log(
                Color::Yellow,
                &format!("🌎 Uploading file: {} to {}", file_name, remote_path_str),
            );

            if let Err(e) = transfer.upload_file(&path, &remote_path_str).await {
                color_log(
                    Color::Red,
                    &format!("❌ Failed to upload {}: {:?}", file_name, e),
                );
            }
        }
    }

    Ok(())
}

async fn ensure_remote_directory(
    transfer: &mut Box<dyn FileTransfer>,
    path: &str,
) -> Result<(), TransferError> {
    color_log(
        Color::BrightMagenta,
        &format!("Attempting to ensure directory: {}", path),
    );

    // Try to change to the directory first
    if transfer.change_directory(path).await.is_ok() {
        color_log(Color::Green, &format!("Directory already exists: {}", path));
        return Ok(());
    }

    // If changing directory failed, create the directory structure
    let mut current_path = String::new();
    for segment in path.split('/') {
        if !segment.is_empty() {
            if !current_path.is_empty() {
                current_path.push('/');
            }
            current_path.push_str(segment);

            color_log(
                Color::Yellow,
                &format!("Checking/Creating directory segment: {}", current_path),
            );

            // Try to change to the directory first
            if transfer.change_directory(&current_path).await.is_err() {
                color_log(
                    Color::BrightCyan,
                    &format!("Creating directory: {}", current_path),
                );

                // Try to create the directory
                match transfer.create_directory(&current_path).await {
                    Ok(_) => {
                        // After creating, try to change into it to verify
                        if transfer.change_directory(&current_path).await.is_err() {
                            color_log(
                                Color::Red,
                                &format!(
                                    "Failed to access newly created directory: {}",
                                    current_path
                                ),
                            );
                            return Err(TransferError::PathError(format!(
                                "Failed to access directory after creation: {}",
                                current_path
                            )));
                        }
                    }
                    Err(e) => {
                        // If directory creation failed, check if it's because it already exists
                        if transfer.change_directory(&current_path).await.is_ok() {
                            color_log(
                                Color::Green,
                                &format!("Directory already exists: {}", current_path),
                            );
                            continue;
                        }
                        color_log(
                            Color::Red,
                            &format!("Failed to create directory {}: {:?}", current_path, e),
                        );
                        return Err(e);
                    }
                }
            }
        }
    }

    Ok(())
}
