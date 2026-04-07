use crate::load_balancer::LoadBalancer;
use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer};
use std::{env, fs, net::SocketAddr, time::Duration};

#[derive(Deserialize)]
pub struct HealthCheckConfig {
    pub path: String,

    #[serde(
        rename = "interval_secs",
        deserialize_with = "deserialize_duration_secs"
    )]
    pub interval: Duration,
}

/// Serde-facing route config, deserialized directly from TOML.
#[derive(Deserialize)]
pub struct RouteConfig {
    pub prefix: String,
    pub backend_addrs: Vec<SocketAddr>,
    pub health_check: HealthCheckConfig,
}

/// Runtime route, built from a `RouteConfig` after deserialization.
pub struct Route {
    pub prefix: String,
    pub backends: Box<dyn LoadBalancer>,
}

#[derive(Deserialize)]
pub struct Config {
    pub listen_addr: SocketAddr,
    pub routes: Vec<RouteConfig>,

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

/// Deserializes a `u64` representing seconds into a `Duration`.
fn deserialize_duration_secs<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
    u64::deserialize(d).map(Duration::from_secs)
}
