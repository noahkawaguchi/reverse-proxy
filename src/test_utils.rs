use anyhow::{Context, Result};

/// Replaces `#[tokio::test]`, not inserting `#[allow(clippy::expect_used)]`.
///
/// Based on the "equivalent code" listed in the docs at
/// <https://docs.rs/tokio/latest/tokio/attr.test.html#using-current-thread-runtime>
pub fn tokio_test<F: Future<Output = Result<()>>>(f: F) -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to set up Tokio runtime for test")?
        .block_on(f)
}
