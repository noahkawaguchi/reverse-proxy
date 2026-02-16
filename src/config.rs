use anyhow::{Context, Result};
use serde::Deserialize;
use std::{env, fs, net::SocketAddr};

#[derive(Deserialize)]
pub struct Config {
    pub listen_addr: SocketAddr,
    pub backend_addr: SocketAddr,
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = env::var("REVERSE_PROXY_CONFIG_PATH")
            .unwrap_or_else(|_| String::from("reverse-proxy.toml"));

        let contents = fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read config file at {config_path}"))?;

        toml::from_str(&contents).context("failed to parse config file")
    }
}
