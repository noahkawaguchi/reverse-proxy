mod backend;
mod config;
mod health;
mod logger;
mod proxy;
mod round_robin;
mod server;
mod shutdown_signal;

#[cfg(test)]
mod test_utils;

use crate::config::Config;
use anyhow::Result;
use tokio::net::TcpListener;
use tracing::level_filters::LevelFilter;

fn main() -> Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async {
            logger::init_with_default(LevelFilter::INFO)?;

            let config = Config::load()?;
            let listener = TcpListener::bind(config.listen_addr).await?;

            server::run(config, listener, shutdown_signal::listen()?).await
        })
}
