use crate::misc::errors::transfer::TransferError;
use async_ftp::FtpStream;
use dotenv::dotenv;
use ssh2::Session;
use std::path::Path;
use std::env;
use async_recursion::async_recursion;
use async_trait::async_trait;
use colored::Color;
use crate::misc::util::{color_log, get_credentials};

#[derive(Clone)]
pub(crate) struct Credentials {
    pub host: String,
    pub username: String,
    pub password: String,
    pub port: u16,
    pub remote_dir: String,
}

#[async_trait]
pub trait FileTransfer {
    async fn begin(credentials: &Credentials) -> Result<Self, TransferError>
    where
        Self: Sized;
    async fn change_directory(&mut self, path: &str) -> Result<(), TransferError>;
    async fn create_directory(&mut self, path: &str) -> Result<(), TransferError>;
    async fn upload_file(&mut self, local_path: &Path, remote_filename: &str) -> Result<(), TransferError>;
    async fn disconnect(&mut self) -> Result<(), TransferError>;
}

#[async_trait]
impl FileTransfer for FtpStream {
    async fn begin(credentials: &Credentials) -> Result<Self, TransferError> {
        let address = format!("{}:{}", credentials.host, credentials.port);
        let mut ftp_stream = FtpStream::connect(address).await?;
        ftp_stream.login(&credentials.username, &credentials.password).await?;
        // Set transfer type to Binary
        ftp_stream.transfer_type(async_ftp::types::FileType::Binary).await?;
        Ok(ftp_stream)
    }

    async fn change_directory(&mut self, path: &str) -> Result<(), TransferError> {
        self.cwd(path).await.map_err(TransferError::from)
    }

    async fn create_directory(&mut self, path: &str) -> Result<(), TransferError> {
        self.mkdir(path).await.map_err(TransferError::from)
    }

    async fn upload_file(&mut self, local_path: &Path, remote_filename: &str) -> Result<(), TransferError> {
        use tokio::fs::File;
        let mut local_file = File::open(local_path).await?;
        self.put(remote_filename, &mut local_file).await.map_err(TransferError::from)
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
            let tcp = TcpStream::connect(format!("{}:{}", credentials.host, credentials.port))?;
            let mut session = Session::new()?;
            session.set_tcp_stream(tcp);
            session.handshake()?;
            session.userauth_password(&credentials.username, &credentials.password)?;
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
            sftp.mkdir(Path::new(&path), 0o755).map_err(TransferError::from)
        })
        .await??;
        Ok(())
    }

    async fn upload_file(&mut self, local_path: &Path, remote_filename: &str) -> Result<(), TransferError> {
        let session = self.session.clone();
        let local_path = local_path.to_path_buf();
        let remote_filename = remote_filename.to_string();

        tokio::task::spawn_blocking(move || -> Result<(), TransferError> {
            let sftp = session.sftp().map_err(TransferError::SshError)?;
            let remote_file_path = Path::new(&remote_filename);
            let mut remote_file = sftp.create(remote_file_path).map_err(TransferError::SshError)?;
            let mut local_file = std::fs::File::open(local_path).map_err(TransferError::IoError)?;
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

pub async fn get_transfer(protocol: &str, credentials: &Credentials) -> Result<Box<dyn FileTransfer>, TransferError> {
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
    let credentials = get_credentials(&protocol)?;
    
    let mut transfer = get_transfer(&protocol, &credentials).await?;
    
    upload_directory(&mut transfer, Path::new(build_folder), &credentials.remote_dir).await?;
    transfer.disconnect().await?;
    
    Ok(())
}

#[async_recursion(?Send)]
pub async fn upload_directory(
    transfer: &mut Box<dyn FileTransfer>,
    local_path: &Path,
    remote_path: &str,
) -> Result<(), TransferError> {
    if transfer.change_directory(remote_path).await.is_err() {
        transfer.create_directory(remote_path).await?;
        transfer.change_directory(remote_path).await?;
    }

    let mut entries = tokio::fs::read_dir(local_path).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let file_name = path.file_name().unwrap().to_string_lossy().to_string();

        if path.is_dir() {
            color_log(Color::BrightYellow, &format!("📁 Uploading {}", file_name));
            // Recursive call
            upload_directory(
                transfer,
                &path,
                &file_name,
            )
            .await?;
            transfer.change_directory("..").await?;
        } else {
            color_log(Color::Yellow, &format!("🌎 Uploading {}", file_name));
            transfer.upload_file(&path, &file_name).await?;
        }
    }

    transfer.change_directory("..").await?;
    Ok(())
}