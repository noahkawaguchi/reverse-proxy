mod config;
mod logger;
mod proxy;

use crate::config::Config;
use anyhow::Result;
use hyper::{server::conn::http1 as server_http1, service::service_fn};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tracing::{error, info, level_filters::LevelFilter};

async fn async_main() -> Result<()> {
    logger::init_with_default(LevelFilter::INFO)?;
    let config = Config::load()?;

    let listener = TcpListener::bind(config.listen_addr).await?;
    info!(
        "Listening on {}, forwarding to {}",
        config.listen_addr, config.backend_addr,
    );

    loop {
        let (stream, client_addr) = listener.accept().await?;
        let io = TokioIo::new(stream);

        tokio::spawn(async move {
            if let Err(e) = server_http1::Builder::new()
                .serve_connection(
                    io,
                    service_fn(|req| proxy::forward(req, client_addr, config.backend_addr)),
                )
                .await
            {
                error!(
                    "Error forwarding request from {client_addr} to {}: {e}",
                    config.backend_addr
                );
            }
        });
    }
}

fn main() -> Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async_main())
}
