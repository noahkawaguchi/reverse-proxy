use {
    anyhow::{Result, anyhow},
    tracing::{debug, level_filters::LevelFilter},
    tracing_subscriber::EnvFilter,
};

/// Installs a global tracing subscriber that defaults to `default_level` unless overridden by the
/// `RUST_LOG` environment variable.
///
/// Also checks for the case where `RUST_LOG` is set to something other than "OFF" (case
/// insensitive), but logging is off, printing a warning to stderr if so.
///
/// # Errors
///
/// Returns `Err` if initializing the subscriber was unsuccessful, likely because there was already
/// a global subscriber installed.
pub fn init_with_default(default_level: LevelFilter) -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(default_level.into())
                .from_env_lossy(),
        )
        .try_init()
        .map_err(|e| anyhow!("Failed to initialize tracing subscriber: {e}"))?;

    // Both `.from_env()` and `.from_env_lossy()` seem to silently disable logging if RUST_LOG is
    // set to a typo/bogus value, so check if the "error" level is disabled but `RUST_LOG` is set to
    // something other than "OFF" (case insensitive).
    if !tracing::enabled!(tracing::Level::ERROR)
        && let Ok(val) = std::env::var(EnvFilter::DEFAULT_ENV)
        && !val.eq_ignore_ascii_case(&LevelFilter::OFF.to_string())
    {
        eprintln!(
            "Warning: Logging is off but environment variable {} is: {val}",
            EnvFilter::DEFAULT_ENV
        );
    }

    debug!("Current most verbose log level: {}", LevelFilter::current());

    Ok(())
}
