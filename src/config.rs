use crate::round_robin::RoundRobin;
use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer};
use std::{env, fs, net::SocketAddr, time::Duration};

/// Deserializes a `u64` representing seconds into a `Duration`.
fn deserialize_duration_secs<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
    u64::deserialize(d).map(Duration::from_secs)
}

#[derive(Deserialize)]
pub struct Route {
    pub prefix: String,
    pub backend_addrs: RoundRobin,
}

#[derive(Deserialize)]
pub struct Config {
    pub listen_addr: SocketAddr,
    pub routes: Vec<Route>,

    #[serde(deserialize_with = "deserialize_duration_secs")]
    pub shutdown_timeout: Duration,
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
