use crate::contexts::{js, scss};
use crate::misc::util::color_log;
use colored::Color;
use serde::Deserialize;
use std::error::Error;
use std::fs;
use std::io::ErrorKind;
use tokio::sync::OnceCell;

const DEFAULT_CONFIG: &str = include_str!("../../config/turtle-devtools.toml");

#[derive(Deserialize, Clone)]
pub(crate) struct Config {
    #[serde(default = "output_folder")]
    pub output_folder: String,
    pub contexts: Contexts,
}

#[derive(Deserialize, Clone)]
pub(crate) struct Contexts {
    #[serde(default)]
    pub scss: Option<scss::Context>,
    #[serde(default)]
    pub js: Option<js::Context>,
}

impl Config {
    pub(crate) async fn load() -> Result<Config, Box<dyn Error>> {
        let config: Config = match fs::read_to_string("turtle-devtools.toml") {
            Ok(content) => toml::from_str(&content)?,
            Err(err) => {
                match err.kind() {
                    ErrorKind::NotFound => {}
                    _ => color_log(Color::BrightRed, format!("Failed to load config: {}", err)),
                }

                color_log(Color::BrightGreen, "Loading default configuration");
                Config::default()
            }
        };

        CONFIG.get_or_init(|| async { config.clone() }).await;
        Ok(config)
    }
}

impl Default for Config {
    fn default() -> Self {
        toml::from_str(DEFAULT_CONFIG).unwrap_or_else(|e| {
            panic!("Failed to parse default config: {}", e);
        })
    }
}

pub static CONFIG: OnceCell<Config> = OnceCell::const_new();

fn output_folder() -> String {
    "build".to_string()
}
