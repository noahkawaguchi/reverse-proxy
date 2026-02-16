mod config;
mod logger;
mod proxy;
mod shutdown_signal;

use crate::config::Config;
use anyhow::Result;
use hyper::{server::conn::http1 as server_http1, service::service_fn};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tracing::{error, info, level_filters::LevelFilter};

async fn run(config: Config, shutdown_signal: impl Future<Output = ()>) -> Result<()> {
    let listener = TcpListener::bind(config.listen_addr).await?;

    info!(
        "Listening on {}, forwarding to {}",
        config.listen_addr, config.backend_addr,
    );

    tokio::pin!(shutdown_signal);

    loop {
        tokio::select! {
            accept_res = listener.accept() => spawn_service_task(accept_res?, config.backend_addr),

            () = &mut shutdown_signal => break Ok(()),
        }
    }
}

fn spawn_service_task((stream, client_addr): (TcpStream, SocketAddr), backend_addr: SocketAddr) {
    tokio::spawn(async move {
        if let Err(e) = server_http1::Builder::new()
            .serve_connection(
                TokioIo::new(stream),
                service_fn(|req| proxy::forward(req, client_addr, backend_addr)),
            )
            .await
        {
            error!("Error forwarding request from {client_addr} to {backend_addr}: {e}");
        }
    });
}

fn main() -> Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async {
            logger::init_with_default(LevelFilter::INFO)?;

            run(Config::load()?, shutdown_signal::listen()?).await
        })
}
