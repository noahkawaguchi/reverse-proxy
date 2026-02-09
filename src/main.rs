use anyhow::Result;
use hyper::{
    Request, Response, body::Incoming, client::conn::http1 as client_http1,
    server::conn::http1 as server_http1, service::service_fn,
};
use hyper_util::rt::TokioIo;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tokio::net::{TcpListener, TcpStream};

const LISTEN_ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 3000);
const BACKEND_ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8000);

async fn proxy(req: Request<Incoming>) -> Result<Response<Incoming>> {
    let io = TokioIo::new(TcpStream::connect(BACKEND_ADDR).await?);
    let (mut sender, conn) = client_http1::handshake(io).await?;

    tokio::spawn(async move {
        if let Err(e) = conn.await {
            eprintln!("Backend connection error: {e}");
        }
    });

    sender.send_request(req).await.map_err(Into::into)
}

async fn async_main() -> Result<()> {
    let listener = TcpListener::bind(LISTEN_ADDR).await?;
    println!("Listening on {LISTEN_ADDR}, forwarding to {BACKEND_ADDR}");

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);

        tokio::spawn(async move {
            if let Err(e) = server_http1::Builder::new()
                .serve_connection(io, service_fn(proxy))
                .await
            {
                eprintln!("Connection error: {e}");
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
