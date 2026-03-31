use crate::{
    backend::Backend,
    config::{Config, Route},
    health::HealthChecker,
    proxy,
    round_robin::RoundRobin,
};
use anyhow::Result;
use hyper::{server::conn::http1 as server_http1, service::service_fn};
use hyper_util::rt::TokioIo;
use std::sync::Arc;
use tokio::{net::TcpListener, sync::watch, task::JoinSet, time::timeout};
use tracing::{error, info, warn};

pub async fn run(
    config: Config,
    listener: TcpListener,
    shutdown_signal: impl Future<Output = ()>,
) -> Result<()> {
    let mut runtime_routes = Vec::with_capacity(config.routes.len());

    for route_config in config.routes {
        let backends = route_config
            .backend_addrs
            .into_iter()
            .map(Backend::healthy) // Assume backends are healthy on startup
            .collect::<Vec<_>>()
            .into();

        let backend_addrs = RoundRobin::init(Arc::clone(&backends))?;

        tokio::spawn(
            HealthChecker::new(
                backends,
                route_config.health_check.path,
                route_config.health_check.interval,
            )
            .run(),
        );

        runtime_routes.push(Route { prefix: route_config.prefix, backend_addrs });
    }

    let routes = Arc::<[_]>::from(runtime_routes);

    info!(
        "Listening on {} with {} route(s)",
        listener.local_addr()?,
        routes.len()
    );

    let (shutdown_tx, _) = watch::channel(false);
    let mut join_set = JoinSet::new();

    tokio::pin!(shutdown_signal);

    loop {
        tokio::select! {
            () = &mut shutdown_signal => break,

            accept_res = listener.accept() => {
                let (stream, client_addr) = accept_res?;
                let mut shutdown_rx = shutdown_tx.subscribe();
                let conn_routes = Arc::clone(&routes);

                join_set.spawn(async move {
                    let conn = server_http1::Builder::new().serve_connection(
                        TokioIo::new(stream),
                        service_fn(move |req| {
                            proxy::forward(req, client_addr, Arc::clone(&conn_routes))
                        }),
                    );

                    tokio::pin!(conn);

                    loop {
                        tokio::select! {
                            conn_res = conn.as_mut() => {
                                if let Err(e) = conn_res {
                                    error!("Error serving connection from {client_addr}: {e}");
                                }

                                break;
                            }

                            shutdown_res = shutdown_rx.changed() => {
                                if let Err(e) = shutdown_res {
                                    error!(
                                        "Shutdown channel closed with current value seen, \
                                        sending graceful shutdown anyway: {e}"
                                    );
                                }

                                conn.as_mut().graceful_shutdown();
                            }
                        }
                    }
                });
            }
        }
    }

    let open_count = join_set.len();

    if open_count > 0 {
        info!("Draining {open_count} open connection(s)...");

        if let Err(e) = shutdown_tx.send(true) {
            warn!("Shutdown channel closed: {e}");
        } else if timeout(config.shutdown_timeout, async {
            while join_set.join_next().await.is_some() {}
        })
        .await
        .is_ok()
        {
            info!("All connections closed within timeout");
        } else {
            warn!(
                "Shutdown timeout exceeded, dropping {} remaining connections",
                join_set.len()
            );
        }
    }

    info!("Graceful shutdown process complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{Config, HealthCheckConfig, RouteConfig},
        test_utils::{localhost_addr, tokio_test},
    };
    use http_body_util::{Empty, Full};
    use hyper::{
        Request, Response, StatusCode, body::Bytes, client::conn::http1 as client_http1,
        server::conn::http1 as server_http1, service::service_fn,
    };
    use hyper_util::rt::TokioIo;
    use std::{convert::Infallible, net::SocketAddr, time::Duration};
    use tokio::{
        net::{TcpListener, TcpStream},
        sync::oneshot,
        time::Instant,
    };

    fn new_test_config(backend_addr: SocketAddr, shutdown_timeout: Duration) -> Config {
        Config {
            listen_addr: localhost_addr(0),
            routes: vec![RouteConfig {
                prefix: String::from("/"),
                backend_addrs: vec![backend_addr],
                health_check: HealthCheckConfig {
                    path: String::from("/"),
                    interval: Duration::from_secs(10),
                },
            }],
            shutdown_timeout,
        }
    }

    /// Spawns a backend that responds to each request after `response_delay`.
    async fn spawn_test_backend(response_delay: Duration) -> Result<SocketAddr> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;

        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else { break };

                tokio::spawn(async move {
                    let _ = server_http1::Builder::new()
                        .serve_connection(
                            TokioIo::new(stream),
                            service_fn(|_| async move {
                                tokio::time::sleep(response_delay).await;
                                Ok::<_, Infallible>(Response::new(Full::new(Bytes::new())))
                            }),
                        )
                        .await;
                });
            }
        });

        Ok(addr)
    }

    /// Sends a GET request to `proxy_addr` and returns the response status code.
    async fn send_request(proxy_addr: SocketAddr) -> Result<StatusCode> {
        let io = TokioIo::new(TcpStream::connect(proxy_addr).await?);
        let (mut sender, conn) = client_http1::handshake(io).await?;

        tokio::spawn(async move {
            let _ = conn.await;
        });

        let req = Request::builder()
            .uri(format!("http://{proxy_addr}/"))
            .header("host", proxy_addr.to_string())
            .body(Empty::<Bytes>::new())?;

        Ok(sender.send_request(req).await?.status())
    }

    #[test]
    fn in_flight_request_completes_before_shutdown() -> Result<()> {
        tokio_test(async {
            let backend_addr = spawn_test_backend(Duration::from_millis(100)).await?;

            let proxy_listener = TcpListener::bind("127.0.0.1:0").await?;
            let proxy_addr = proxy_listener.local_addr()?;
            let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
            let config = new_test_config(backend_addr, Duration::from_secs(2));

            let proxy = tokio::spawn(run(config, proxy_listener, async move {
                let _ = shutdown_rx.await;
            }));

            let request = tokio::spawn(send_request(proxy_addr));

            // Let the request reach the backend before triggering shutdown
            tokio::time::sleep(Duration::from_millis(50)).await;

            // Trigger shutdown while the backend is still processing (it sleeps 100ms total)
            let _ = shutdown_tx.send(());

            // The in-flight request should still complete successfully
            assert_eq!(request.await??, StatusCode::OK);
            proxy.await??;

            Ok(())
        })
    }

    #[test]
    fn exits_after_timeout_with_hung_connection() -> Result<()> {
        tokio_test(async {
            let backend_addr = spawn_test_backend(Duration::from_secs(60)).await?;

            let proxy_listener = TcpListener::bind("127.0.0.1:0").await?;
            let proxy_addr = proxy_listener.local_addr()?;
            let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
            let config = new_test_config(backend_addr, Duration::from_millis(100));

            let proxy = tokio::spawn(run(config, proxy_listener, async move {
                let _ = shutdown_rx.await;
            }));

            // Start a request that will be held at the slow backend
            tokio::spawn(send_request(proxy_addr));

            // Let the request reach the backend before triggering shutdown
            tokio::time::sleep(Duration::from_millis(50)).await;

            let start = Instant::now();
            let _ = shutdown_tx.send(());
            proxy.await??;

            let elapsed = start.elapsed();

            assert!(
                elapsed < Duration::from_millis(150),
                "expected proxy to exit near the 100ms timeout, took {elapsed:?}"
            );

            Ok(())
        })
    }

    #[test]
    fn returns_502_when_all_backends_unhealthy() -> Result<()> {
        tokio_test(async {
            // Bind then drop to get an address with nothing listening on it
            let reserved = TcpListener::bind("127.0.0.1:0").await?;
            let backend_addr = reserved.local_addr()?;
            drop(reserved);

            let proxy_listener = TcpListener::bind("127.0.0.1:0").await?;
            let proxy_addr = proxy_listener.local_addr()?;
            let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
            let config = new_test_config(backend_addr, Duration::from_secs(1));

            let proxy = tokio::spawn(run(config, proxy_listener, async move {
                let _ = shutdown_rx.await;
            }));

            // Wait for the health checker to mark the backend unhealthy
            tokio::time::sleep(Duration::from_millis(200)).await;

            assert_eq!(send_request(proxy_addr).await?, StatusCode::BAD_GATEWAY);

            let _ = shutdown_tx.send(());
            proxy.await??;

            Ok(())
        })
    }
}
