use tokio::sync::OnceCell;
use std::error::Error;
use std::fs;
use serde::Deserialize;
use crate::contexts::{scss, js};

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
        let config_content = fs::read_to_string("turtle-devtools.toml")?;
        let config: Config = toml::from_str(&config_content)?;
        CONFIG.get_or_init(|| async { config.clone() }).await;
        Ok(config)
    }

}

pub static CONFIG: OnceCell<Config> = OnceCell::const_new();

fn output_folder() -> String {
    "build".to_string()
}