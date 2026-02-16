use anyhow::Result;
use tracing::{info, warn};

/// Creates Unix signal handlers that listen for SIGINT and SIGTERM.
///
/// # Errors
///
/// Returns `Err` for errors installing the signal handlers, but logs and does not return errors
/// receiving the signals.
#[cfg(unix)]
pub fn listen() -> Result<impl Future<Output = ()>> {
    use tokio::signal::unix;

    let mut sigint = unix::signal(unix::SignalKind::interrupt())?;
    let mut sigterm = unix::signal(unix::SignalKind::terminate())?;

    Ok(async move {
        tokio::select! {
            v = sigint.recv() => {
                if v == Some(()) {
                    info!("SIGINT received, shutting down...");
                } else {
                    warn!("SIGINT stream ended unexpectedly, shutting down...");
                }
            }
            v = sigterm.recv() => {
                if v == Some(()) {
                    info!("SIGTERM received, shutting down...");
                } else {
                    warn!("SIGTERM stream ended unexpectedly, shutting down...");
                }
            }
        }
    })
}

/// Creates a cross-platform signal handler that listens for Ctrl+C.
///
/// # Errors
///
/// Does not return `Err`. This function is only wrapped in `Result` to match the Unix version.
/// Errors receiving Ctrl+C are logged, but not returned.
#[allow(clippy::unnecessary_wraps)]
#[cfg(not(unix))]
pub fn listen() -> Result<impl Future<Output = ()>> {
    Ok(async {
        match tokio::signal::ctrl_c().await {
            Ok(()) => info!("Ctrl+C received, shutting down..."),
            Err(e) => warn!("Ctrl+C handler error, shutting down: {e}"),
        }
    })
}
