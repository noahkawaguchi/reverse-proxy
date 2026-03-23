use anyhow::{Context, Result, bail};
use serde::{Deserialize, Deserializer};
use std::{
    env, fs,
    net::SocketAddr,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

/// Deserializes a `u64` representing seconds into a `Duration`.
fn deserialize_duration_secs<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
    u64::deserialize(d).map(Duration::from_secs)
}

/// One or more backend addresses to be cycled through for round robin load balancing.
#[derive(Deserialize)]
#[serde(try_from = "Vec<SocketAddr>")]
pub struct RoundRobin {
    addrs: Vec<SocketAddr>,
    counter: AtomicUsize,
}

impl RoundRobin {
    pub fn next_addr(&self) -> SocketAddr {
        self.addrs[self.counter.fetch_add(1, Ordering::Relaxed) % self.addrs.len()]
    }
}

impl TryFrom<Vec<SocketAddr>> for RoundRobin {
    type Error = anyhow::Error;

    fn try_from(addrs: Vec<SocketAddr>) -> Result<Self, Self::Error> {
        if addrs.is_empty() {
            bail!("route must have at least one backend address");
        }

        Ok(Self { addrs, counter: AtomicUsize::new(0) })
    }
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
