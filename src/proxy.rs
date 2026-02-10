use anyhow::Result;
use hyper::{
    Request, Response,
    body::Incoming,
    client::conn::http1 as client_http1,
    header::{
        CONNECTION, HOST, HeaderMap, HeaderName, PROXY_AUTHENTICATE, PROXY_AUTHORIZATION, TE,
        TRAILER, TRANSFER_ENCODING, UPGRADE,
    },
};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tracing::error;

const X_FORWARDED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");

/// The eight standard hop-by-hop headers.
const HOP_BY_HOP_HEADERS: [HeaderName; 8] = [
    CONNECTION,
    HeaderName::from_static("keep-alive"),
    PROXY_AUTHENTICATE,
    PROXY_AUTHORIZATION,
    TE,
    TRAILER,
    TRANSFER_ENCODING,
    UPGRADE,
];

pub async fn forward(
    req: Request<Incoming>,
    client_addr: SocketAddr,
    backend_addr: SocketAddr,
) -> Result<Response<Incoming>> {
    let req = prepare_request(req, client_addr, backend_addr)?;
    let io = TokioIo::new(TcpStream::connect(backend_addr).await?);
    let (mut sender, conn) = client_http1::handshake(io).await?;

    tokio::spawn(async move {
        if let Err(e) = conn.await {
            error!("Connection error for backend {backend_addr}: {e}");
        }
    });

    let resp = sender.send_request(req).await?;
    Ok(prepare_response(resp))
}

fn prepare_request<B>(
    mut req: Request<B>,
    client_addr: SocketAddr,
    backend_addr: SocketAddr,
) -> Result<Request<B>> {
    let headers = req.headers_mut();
    let client_ip = client_addr.ip();

    let xff = headers
        .get(X_FORWARDED_FOR)
        .and_then(|v| v.to_str().ok())
        .map_or_else(
            || client_ip.to_string(),
            |existing| format!("{existing}, {client_ip}"),
        );

    strip_hop_by_hop_headers(headers);

    headers.insert(X_FORWARDED_FOR, xff.parse()?);
    headers.insert(HOST, backend_addr.to_string().parse()?);

    Ok(req)
}

fn prepare_response<B>(mut resp: Response<B>) -> Response<B> {
    strip_hop_by_hop_headers(resp.headers_mut());
    resp
}

/// Removes the standard hop-by-hop headers as well as any others named in the connection header.
fn strip_hop_by_hop_headers(headers: &mut HeaderMap) {
    let extra: Vec<_> = headers
        .get(CONNECTION)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(',').map(|s| s.trim().to_lowercase()).collect())
        .unwrap_or_default();

    for name in HOP_BY_HOP_HEADERS {
        headers.remove(name);
    }

    for name in extra {
        headers.remove(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::header::HeaderValue;
    use std::net::{IpAddr, Ipv4Addr};

    const CLIENT_ADDR: SocketAddr =
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), 12345);
    const BACKEND_ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8000);

    #[test]
    fn strip_removes_standard_hop_by_hop_headers() -> Result<()> {
        let mut headers = HeaderMap::new();

        headers.insert("connection", "keep-alive".parse()?);
        headers.insert("keep-alive", "timeout=5".parse()?);
        headers.insert("transfer-encoding", "chunked".parse()?);
        headers.insert("content-type", "text/html".parse()?);

        strip_hop_by_hop_headers(&mut headers);

        assert!(headers.get("connection").is_none());
        assert!(headers.get("keep-alive").is_none());
        assert!(headers.get("transfer-encoding").is_none());
        assert!(headers.get("content-type").is_some());

        Ok(())
    }

    #[test]
    fn strip_removes_headers_named_in_connection() -> Result<()> {
        let mut headers = HeaderMap::new();

        headers.insert("connection", "x-custom, x-another".parse()?);
        headers.insert("x-custom", "foo".parse()?);
        headers.insert("x-another", "bar".parse()?);
        headers.insert("x-keep", "baz".parse()?);

        strip_hop_by_hop_headers(&mut headers);

        assert!(headers.get("x-custom").is_none());
        assert!(headers.get("x-another").is_none());
        assert!(headers.get("x-keep").is_some());

        Ok(())
    }

    #[test]
    fn prepare_request_sets_xff_when_absent() -> Result<()> {
        let req = Request::builder().body(())?;
        let req = prepare_request(req, CLIENT_ADDR, BACKEND_ADDR)?;

        assert_eq!(
            req.headers()
                .get("x-forwarded-for")
                .map(HeaderValue::as_bytes),
            Some(b"192.168.1.100".as_slice()),
        );

        Ok(())
    }

    #[test]
    fn prepare_request_appends_to_existing_xff() -> Result<()> {
        let req = Request::builder()
            .header("x-forwarded-for", "10.0.0.1")
            .body(())?;
        let req = prepare_request(req, CLIENT_ADDR, BACKEND_ADDR)?;

        assert_eq!(
            req.headers()
                .get("x-forwarded-for")
                .map(HeaderValue::as_bytes),
            Some(b"10.0.0.1, 192.168.1.100".as_slice()),
        );

        Ok(())
    }

    #[test]
    fn prepare_request_rewrites_host_to_backend() -> Result<()> {
        let req = Request::builder()
            .header(HOST, "original.example.com")
            .body(())?;
        let req = prepare_request(req, CLIENT_ADDR, BACKEND_ADDR)?;

        assert_eq!(
            req.headers().get(HOST).map(HeaderValue::as_bytes),
            Some(b"127.0.0.1:8000".as_slice()),
        );

        Ok(())
    }

    #[test]
    fn prepare_request_strips_hop_by_hop() -> Result<()> {
        let req = Request::builder()
            .header("connection", "keep-alive")
            .header("keep-alive", "timeout=5")
            .header("x-real-header", "preserved")
            .body(())?;
        let req = prepare_request(req, CLIENT_ADDR, BACKEND_ADDR)?;

        assert!(req.headers().get("connection").is_none());
        assert!(req.headers().get("keep-alive").is_none());
        assert!(req.headers().get("x-real-header").is_some());

        Ok(())
    }

    #[test]
    fn prepare_response_strips_hop_by_hop() -> Result<()> {
        let resp = Response::builder()
            .header("connection", "close")
            .header("transfer-encoding", "chunked")
            .header("content-type", "text/html")
            .body(())?;
        let resp = prepare_response(resp);

        assert!(resp.headers().get("connection").is_none());
        assert!(resp.headers().get("transfer-encoding").is_none());
        assert!(resp.headers().get("content-type").is_some());

        Ok(())
    }
}
