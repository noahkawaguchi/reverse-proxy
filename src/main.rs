mod logger;
mod proxy;

use anyhow::Result;
use hyper::{server::conn::http1 as server_http1, service::service_fn};
use hyper_util::rt::TokioIo;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tokio::net::TcpListener;
use tracing::{error, info, level_filters::LevelFilter};

const LISTEN_ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 3000);
const BACKEND_ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8000);

async fn async_main() -> Result<()> {
    logger::init_with_default(LevelFilter::INFO)?;

    let listener = TcpListener::bind(LISTEN_ADDR).await?;
    info!("Listening on {LISTEN_ADDR}, forwarding to {BACKEND_ADDR}");

    loop {
        let (stream, client_addr) = listener.accept().await?;
        let io = TokioIo::new(stream);

        tokio::spawn(async move {
            if let Err(e) = server_http1::Builder::new()
                .serve_connection(
                    io,
                    service_fn(|req| proxy::forward(req, client_addr, BACKEND_ADDR)),
                )
                .await
            {
                error!("Error forwarding request from {client_addr} to {BACKEND_ADDR}: {e}");
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
