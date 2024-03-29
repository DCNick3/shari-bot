use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Deserialize, Clone, Debug)]
pub struct Config {
    pub telegram: Telegram,
}

impl Config {
    pub fn load(environment: &str) -> Result<Config> {
        let config = config::Config::builder()
            .add_source(config::File::new("config.yaml", config::FileFormat::Yaml).required(false))
            .add_source(
                config::File::new("config.local.yaml", config::FileFormat::Yaml).required(false),
            )
            .add_source(
                config::File::new(
                    &format!("config.{}.yaml", environment),
                    config::FileFormat::Yaml,
                )
                .required(false),
            )
            .add_source(
                config::File::new(
                    &format!("config.{}.local.yaml", environment),
                    config::FileFormat::Yaml,
                )
                .required(false),
            )
            .add_source(
                config::Environment::with_prefix("config")
                    .prefix_separator("_")
                    .separator("__")
                    .list_separator(","),
            )
            .build()
            .context("Building the config file")?;

        config
            .try_deserialize()
            .context("Deserializing config structure failed")
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct Telegram {
    pub session_storage: Option<String>,
    pub api_id: i32,
    pub api_hash: String,
    pub account: TelegramAccount,
}
#[derive(Deserialize, Clone, Debug)]
#[serde(tag = "type")]
pub enum TelegramAccount {
    PreparedSession {
        #[serde(with = "hex_serde")]
        session: Vec<u8>,
    },
    Bot {
        token: String,
    },
    User {
        phone: String,
    },
}
