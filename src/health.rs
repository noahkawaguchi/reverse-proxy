use {
    crate::backend::Backend,
    http_body_util::Empty,
    hyper::{Request, body::Bytes, client::conn::http1 as client_http1, header::HOST},
    hyper_util::rt::TokioIo,
    std::{net::SocketAddr, sync::Arc, time::Duration},
    tokio::net::TcpStream,
    tracing::{info, warn},
};

/// The amount of time to wait for a response when checking a backend's health.
const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(5);

pub struct HealthChecker {
    backends: Vec<Arc<Backend>>,
    path: String,
    interval: Duration,
}

impl HealthChecker {
    pub const fn new(backends: Vec<Arc<Backend>>, path: String, interval: Duration) -> Self {
        Self { backends, path, interval }
    }

    pub async fn run(self) -> ! {
        loop {
            for backend in &self.backends {
                let is_healthy = self.check_backend(backend.addr()).await;
                let was_healthy = backend.is_healthy();

                if is_healthy != was_healthy {
                    if is_healthy {
                        info!("Backend {} is now healthy", backend.addr());
                    } else {
                        warn!("Backend {} is now unhealthy", backend.addr());
                    }

                    backend.set_health(is_healthy);
                }
            }

            tokio::time::sleep(self.interval).await;
        }
    }

    async fn check_backend(&self, addr: SocketAddr) -> bool {
        let check_fut = async {
            let Ok(stream) = TcpStream::connect(addr).await else { return false };
            let Ok((mut sender, conn)) = client_http1::handshake(TokioIo::new(stream)).await else {
                return false;
            };

            tokio::spawn(async {
                if let Err(e) = conn.await {
                    warn!("{e}");
                }
            });

            let Ok(req) = Request::builder()
                .uri(&self.path)
                .header(HOST, addr.to_string())
                .body(Empty::<Bytes>::new())
            else {
                return false;
            };

            sender
                .send_request(req)
                .await
                .is_ok_and(|resp| resp.status().is_success())
        };

        tokio::time::timeout(HEALTH_CHECK_TIMEOUT, check_fut)
            .await
            .is_ok_and(|result| result)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::test_utils::tokio_test,
        anyhow::Result,
        http_body_util::Full,
        hyper::{Response, StatusCode, server::conn::http1 as server_http1, service::service_fn},
        std::{assert_matches, convert::Infallible},
        tokio::net::TcpListener,
    };

    /// Spawns an HTTP backend that responds to every request with `status`.
    async fn spawn_test_backend(status: StatusCode) -> Result<SocketAddr> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;

        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else { break };

                tokio::spawn(async move {
                    assert_matches!(
                        server_http1::Builder::new()
                            .serve_connection(
                                TokioIo::new(stream),
                                service_fn(async |_| {
                                    let resp = Response::builder()
                                        .status(status)
                                        .body(Full::new(Bytes::new()))
                                        .unwrap_or_else(|_| Response::new(Full::new(Bytes::new())));
                                    Ok::<_, Infallible>(resp)
                                }),
                            )
                            .await,
                        Ok(())
                    );
                });
            }
        });

        Ok(addr)
    }

    fn make_checker(addr: SocketAddr, starts_healthy: bool) -> (HealthChecker, Arc<Backend>) {
        let backend = Arc::new(if starts_healthy {
            Backend::healthy(addr)
        } else {
            Backend::unhealthy(addr)
        });

        let checker = HealthChecker::new(
            vec![Arc::clone(&backend)],
            String::from("/"),
            Duration::from_millis(20),
        );

        (checker, backend)
    }

    #[test]
    fn marks_backend_unhealthy_when_unreachable() -> Result<()> {
        tokio_test(async {
            let listener = TcpListener::bind("127.0.0.1:0").await?;
            let unreachable_addr = listener.local_addr()?;
            drop(listener);

            let (checker, backend) = make_checker(unreachable_addr, true);
            tokio::spawn(checker.run());
            tokio::time::sleep(Duration::from_millis(200)).await;

            assert!(!backend.is_healthy());
            Ok(())
        })
    }

    #[test]
    fn marks_backend_healthy_when_responding_with_2xx() -> Result<()> {
        tokio_test(async {
            let backend_addr = spawn_test_backend(StatusCode::OK).await?;
            let (checker, backend) = make_checker(backend_addr, false);

            tokio::spawn(checker.run());
            tokio::time::sleep(Duration::from_millis(200)).await;

            assert!(backend.is_healthy());
            Ok(())
        })
    }

    #[test]
    fn marks_backend_unhealthy_when_responding_with_non_2xx() -> Result<()> {
        tokio_test(async {
            let backend_addr = spawn_test_backend(StatusCode::INTERNAL_SERVER_ERROR).await?;
            let (checker, backend) = make_checker(backend_addr, true);

            tokio::spawn(checker.run());
            tokio::time::sleep(Duration::from_millis(200)).await;

            assert!(!backend.is_healthy());
            Ok(())
        })
    }
}
