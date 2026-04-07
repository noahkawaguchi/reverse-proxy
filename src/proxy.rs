use crate::config::Route;
use anyhow::Result;
use http_body_util::{BodyExt, Empty};
use hyper::{
    Request, Response, StatusCode,
    body::{Bytes, Incoming},
    client::conn::http1 as client_http1,
    header::{
        CONNECTION, HOST, HeaderMap, HeaderName, PROXY_AUTHENTICATE, PROXY_AUTHORIZATION, TE,
        TRAILER, TRANSFER_ENCODING, UPGRADE,
    },
};
use hyper_util::rt::TokioIo;
use std::{net::SocketAddr, sync::Arc};
use tokio::net::TcpStream;
use tracing::{error, warn};

/// A generic `Response` containing a boxed `Body` (may be `Incoming`, `Empty<Bytes>`, etc.).
type BoxBodyResp = hyper::Response<http_body_util::combinators::BoxBody<Bytes, hyper::Error>>;

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
    routes: Arc<[Route]>,
) -> Result<BoxBodyResp> {
    let Some(route) = resolve(req.uri().path(), &routes) else {
        return not_found();
    };

    let Some(guard) = route.backends.next() else {
        return service_unavailable();
    };

    let backend_addr = guard.addr();

    let prepped_req = prepare_request(req, client_addr, backend_addr)?;
    let io = TokioIo::new(TcpStream::connect(backend_addr).await?);
    let (mut sender, conn) = client_http1::handshake(io).await?;

    tokio::spawn(async move {
        if let Err(e) = conn.await {
            error!("Connection error for backend {backend_addr}: {e}");
        }
    });

    sender
        .send_request(prepped_req)
        .await
        .map(prepare_response)
        .map(|resp| resp.map(BodyExt::boxed))
        .map_err(Into::into)
}

/// Determines the route whose prefix is the longest match for `path` or returns `None` if no route
/// matches.
fn resolve<'a>(path: &str, routes: &'a [Route]) -> Option<&'a Route> {
    routes
        .iter()
        .filter(|route| path.starts_with(route.prefix.as_str()))
        .max_by_key(|route| route.prefix.len())
}

/// Creates a 503 Service Unavailable response with an empty body.
fn service_unavailable() -> Result<BoxBodyResp> {
    Response::builder()
        .status(StatusCode::SERVICE_UNAVAILABLE)
        .body(
            Empty::<Bytes>::new()
                .map_err(|infallible| match infallible {})
                .boxed(),
        )
        .map_err(Into::into)
}

/// Creates a 404 Not Found response with an empty body.
fn not_found() -> Result<BoxBodyResp> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(
            Empty::<Bytes>::new()
                .map_err(|infallible| match infallible {})
                .boxed(),
        )
        .map_err(Into::into)
}

/// Sets X-Forwarded-For to the client IP when absent or appends when present, rewrites Host to the
/// backend address, and strips hop-by-hop headers.
fn prepare_request<B>(
    mut req: Request<B>,
    client_addr: SocketAddr,
    backend_addr: SocketAddr,
) -> Result<Request<B>> {
    let headers = req.headers_mut();
    let client_ip = client_addr.ip();

    let xff = get_str_val(headers, &X_FORWARDED_FOR).map_or_else(
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

/// Removes the standard hop-by-hop headers and any custom headers named in the Connection header.
fn strip_hop_by_hop_headers(headers: &mut HeaderMap) {
    let extra = get_str_val(headers, &CONNECTION)
        .map(|s| {
            s.split(',')
                .map(|s| s.trim().to_lowercase())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    for name in HOP_BY_HOP_HEADERS {
        headers.remove(name);
    }

    for name in extra {
        headers.remove(name);
    }
}

/// Gets the `&str` value of `header_name` from `headers`, returning `None` if the header is not
/// present or the value is not visible ASCII, and warning if present but not visible ASCII.
fn get_str_val<'a>(headers: &'a HeaderMap, header_name: &HeaderName) -> Option<&'a str> {
    headers.get(header_name).and_then(|v| match v.to_str() {
        Ok(s) => Some(s),

        Err(e) => {
            warn!("Failed to parse {header_name} header value as visible ASCII: {e}");
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{backend::Backend, round_robin::RoundRobin, test_utils::localhost_addr};
    use hyper::header::HeaderValue;
    use std::net::{IpAddr, Ipv4Addr};

    const BACKEND_ADDR: SocketAddr = localhost_addr(8000);
    const CLIENT_ADDR: SocketAddr =
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), 12345);

    fn new_test_route(prefix: &str, port: u16) -> Result<Route> {
        Ok(Route {
            prefix: prefix.into(),
            backends: Box::new(RoundRobin::init(
                vec![Backend::healthy(localhost_addr(port))].into(),
            )?),
        })
    }

    #[test]
    fn resolve_returns_none_when_no_routes_match() -> Result<()> {
        let routes = [new_test_route("/api", 8001)?];
        assert!(resolve("/other", &routes).is_none());
        Ok(())
    }

    #[test]
    fn resolve_matches_longest_prefix() -> Result<()> {
        let routes = [new_test_route("/", 8000)?, new_test_route("/api", 8001)?];
        let matched = resolve("/api/v1", &routes)
            .and_then(|route| route.backends.next())
            .map(|g| g.addr().port());

        assert_eq!(matched, Some(8001));
        Ok(())
    }

    #[test]
    fn resolve_falls_back_to_shorter_prefix() -> Result<()> {
        let routes = [new_test_route("/", 8000)?, new_test_route("/api", 8001)?];
        let matched = resolve("/other", &routes)
            .and_then(|route| route.backends.next())
            .map(|g| g.addr().port());

        assert_eq!(matched, Some(8000));
        Ok(())
    }

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
        let orig_req = Request::builder().body(())?;
        let prepped_req = prepare_request(orig_req, CLIENT_ADDR, BACKEND_ADDR)?;

        let xff_val = prepped_req
            .headers()
            .get("x-forwarded-for")
            .map(HeaderValue::as_bytes);

        assert_eq!(xff_val, Some(b"192.168.1.100".as_slice()));

        Ok(())
    }

    #[test]
    fn prepare_request_appends_to_existing_xff() -> Result<()> {
        let orig_req = Request::builder()
            .header("x-forwarded-for", "10.0.0.1")
            .body(())?;

        let prepped_req = prepare_request(orig_req, CLIENT_ADDR, BACKEND_ADDR)?;

        let xff_val = prepped_req
            .headers()
            .get("x-forwarded-for")
            .map(HeaderValue::as_bytes);

        assert_eq!(xff_val, Some(b"10.0.0.1, 192.168.1.100".as_slice()));

        Ok(())
    }

    #[test]
    fn prepare_request_rewrites_host_to_backend() -> Result<()> {
        let orig_req = Request::builder()
            .header(HOST, "original.example.com")
            .body(())?;

        let prepped_req = prepare_request(orig_req, CLIENT_ADDR, BACKEND_ADDR)?;
        let host_val = prepped_req.headers().get(HOST).map(HeaderValue::as_bytes);

        assert_eq!(host_val, Some(b"127.0.0.1:8000".as_slice()));

        Ok(())
    }

    #[test]
    fn prepare_request_strips_hop_by_hop() -> Result<()> {
        let orig_req = Request::builder()
            .header("connection", "keep-alive")
            .header("keep-alive", "timeout=5")
            .header("x-real-header", "preserved")
            .body(())?;

        let prepped_req = prepare_request(orig_req, CLIENT_ADDR, BACKEND_ADDR)?;

        assert!(prepped_req.headers().get("connection").is_none());
        assert!(prepped_req.headers().get("keep-alive").is_none());
        assert!(prepped_req.headers().get("x-real-header").is_some());

        Ok(())
    }

    #[test]
    fn prepare_response_strips_hop_by_hop() -> Result<()> {
        let orig_resp = Response::builder()
            .header("connection", "close")
            .header("transfer-encoding", "chunked")
            .header("content-type", "text/html")
            .body(())?;

        let prepped_resp = prepare_response(orig_resp);

        assert!(prepped_resp.headers().get("connection").is_none());
        assert!(prepped_resp.headers().get("transfer-encoding").is_none());
        assert!(prepped_resp.headers().get("content-type").is_some());

        Ok(())
    }
}
