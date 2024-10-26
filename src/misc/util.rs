use std::collections::HashSet;
use colored::{Color, Colorize};
use std::env;
use crate::misc::errors::transfer::TransferError;
use crate::misc::transfer::Credentials;

pub fn color_log(color: Color, message: &str) {
    let colored_message = colored::ColoredString::from(message);
    
    println!("{}", colored_message.color(color));
}

pub fn get_credentials(protocol: &str) -> Result<Credentials, TransferError> {
    let mut missing_vars = HashSet::new();

    let (host_vars, username_vars, password_vars, port_vars, remote_dir_vars) = match protocol {
        "ssh" => (
            vec!["SSH_HOST", "HOST"],
            vec!["SSH_USERNAME", "USERNAME"],
            vec!["SSH_PASSWORD", "PASSWORD"],
            vec!["SSH_PORT", "PORT"],
            vec!["REMOTE_PATH"],
        ),
        "ftp" => (
            vec!["FTP_HOST", "HOST"],
            vec!["FTP_USERNAME", "USERNAME"],
            vec!["FTP_PASSWORD", "PASSWORD"],
            vec!["FTP_PORT", "PORT"],
            vec!["REMOTE_PATH"],
        ),
        _ => {
            return Err(TransferError::UnsupportedProtocol(protocol.to_string()));
        }
    };

    // Function to get env var from a list of variable names
    fn get_env_var_from_list(var_names: &[&str]) -> Result<String, String> {
        for &var_name in var_names {
            if let Ok(val) = env::var(var_name) {
                return Ok(val);
            }
        }
        // Return the shared (last) variable name as missing
        let last_var = *var_names.last().unwrap();
        Err(last_var.to_string())
    }

    // Collect missing environment variables
    let host = match get_env_var_from_list(&host_vars) {
        Ok(val) => val,
        Err(var) => {
            missing_vars.insert(var);
            String::new()
        }
    };

    let username = match get_env_var_from_list(&username_vars) {
        Ok(val) => val,
        Err(var) => {
            missing_vars.insert(var);
            String::new()
        }
    };

    let password = match get_env_var_from_list(&password_vars) {
        Ok(val) => val,
        Err(var) => {
            missing_vars.insert(var);
            String::new()
        }
    };

    let remote_dir = match get_env_var_from_list(&remote_dir_vars) {
        Ok(val) => val,
        Err(var) => {
            missing_vars.insert(var);
            String::new()
        }
    };

    let port_str = match get_env_var_from_list(&port_vars) {
        Ok(val) => val,
        Err(_) => {
            // Use default ports
            match protocol {
                "ssh" => "22".to_string(),
                "ftp" => "21".to_string(),
                _ => "0".to_string(),
            }
        }
    };
    
    let port = match port_str.parse::<u16>() {
        Ok(val) => val,
        Err(_) => {
            return Err(TransferError::InvalidPort(port_str));
        }
    };

    // If any variables are missing, return an error
    if !missing_vars.is_empty() {
        // Convert HashSet to Vec and sort for consistency
        let mut missing_list = missing_vars.into_iter().collect::<Vec<_>>();
        missing_list.sort();
        return Err(TransferError::MissingEnvironmentVariables(missing_list));
    }

    Ok(Credentials {
        host,
        username,
        password,
        port,
        remote_dir,
    })
}