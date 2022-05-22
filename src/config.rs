#![allow(clippy::use_self)]

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    lazy::SyncOnceCell,
    time::SystemTime,
};

use anyhow::Result;
use figment::{providers::Env, Figment};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use tracing::{level_filters::LevelFilter, warn};

#[must_use]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]

pub enum BotMode {
    Webhook,
    Poll,
}

impl BotMode {
    #[must_use]
    pub const fn is_webhook(&self) -> bool {
        matches!(self, Self::Webhook)
    }

    #[must_use]
    pub const fn is_poll(&self) -> bool {
        matches!(self, Self::Poll)
    }
}

impl Default for BotMode {
    fn default() -> Self {
        Self::Poll
    }
}

mod default {
    use tracing::level_filters::LevelFilter;

    pub const fn log() -> LevelFilter {
        LevelFilter::INFO
    }
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde_as(as = "DisplayFromStr")]
    #[serde(default = "default::log")]
    pub log: LevelFilter,
    pub token: String,
    #[serde(default)]
    pub mode: BotMode,
    pub domain: Option<String>,
    pub debug_chat: Option<i64>,
}

impl Config {
    /// Construct the config from environment with prefix `GOLDEN_AXE_`.
    ///
    /// e.g. `GOLDEN_AXE_TOKEN=123456789:ABCDEFGHIJKLMNOPQRSTUVWXYZ`
    ///
    /// # Additional check
    ///
    /// Config is in good shape iff:
    /// - When `mode` being set to [`BotMode::Webhook`], `domain` is also set.
    ///
    /// Bad config will result in an `Err` being returned.
    ///
    /// # Errors
    /// If any of the required environment variable is not set or not in proper format,
    /// or the config is [not in good shape](#additional-check).
    pub fn from_env() -> Result<Self> {
        Figment::new()
            .merge(Env::prefixed("GOLDEN_AXE_"))
            .extract::<Self>()?
            .ensure_good()
    }

    /// Get or initialize the config.
    ///
    /// # Errors
    /// Failed to [construct the config from env](#method.from_env).
    pub fn try_get<'a>() -> Result<&'a Self> {
        static CELL: SyncOnceCell<Config> = SyncOnceCell::new();
        CELL.get_or_try_init(Self::from_env).map_err(|e| {
            warn!("{}", e);
            anyhow::anyhow!("Failed to initialize config")
        })
    }

    /// Get or initialize the config.
    ///
    /// # Panics
    /// Failed to [construct the config from env](#method.from_env)
    #[must_use]
    pub fn get<'a>() -> &'a Self {
        Self::try_get().unwrap()
    }

    pub fn run_hash<'a>(&self) -> &'a str {
        static CELL: SyncOnceCell<String> = SyncOnceCell::new();
        CELL.get_or_init(|| {
            let mut hasher = DefaultHasher::new();

            self.token.hash(&mut hasher);

            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("Wrong system time config")
                .hash(&mut hasher);
            format!("{:X}", hasher.finish())
        })
    }

    fn ensure_good(self) -> Result<Self> {
        if self.mode.is_webhook() && self.domain.is_none() {
            Err(anyhow::anyhow!(
                "Cannot set bot mode to webhook when domain is not present"
            ))
        } else {
            Ok(self)
        }
    }
}

#[test]
fn test_config() {
    figment::Jail::expect_with(|j| {
        j.set_env("GOLDEN_AXE_LOG", "debug");
        j.set_env("GOLDEN_AXE_TOKEN", "token");
        j.set_env("GOLDEN_AXE_MODE", "webhook");
        j.set_env("GOLDEN_AXE_DOMAIN", "domain");
        j.set_env("GOLDEN_AXE_DEBUG_CHAT", "123");

        assert_eq!(
            Config::from_env().unwrap(),
            Config {
                log: LevelFilter::DEBUG,
                token: "token".to_string(),
                mode: BotMode::Webhook,
                domain: Some("domain".to_string()),
                debug_chat: Some(123),
            }
        );
        Ok(())
    });
}

#[test]
fn test_config_minimal() {
    figment::Jail::expect_with(|j| {
        j.set_env("GOLDEN_AXE_TOKEN", "token");

        assert_eq!(
            Config::from_env().unwrap(),
            Config {
                log: LevelFilter::INFO,
                token: "token".to_string(),
                mode: BotMode::Poll,
                domain: None,
                debug_chat: None,
            }
        );
        Ok(())
    });
}
