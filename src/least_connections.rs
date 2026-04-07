use crate::{
    backend::Backend,
    load_balancer::{BackendGuard, LoadBalancer},
};
use anyhow::{Result, bail};
use rand::prelude::IndexedRandom;
use std::sync::Arc;

/// One or more backends selected by fewest active connections.
/// Skips backends whose `healthy` flag is `false`. Breaks ties randomly.
pub struct LeastConnections {
    backends: Arc<[Backend]>,
}

impl LeastConnections {
    pub fn init(backends: Arc<[Backend]>) -> Result<Self> {
        if backends.is_empty() {
            bail!("route must have at least one backend address");
        }

        Ok(Self { backends })
    }
}

impl LoadBalancer for LeastConnections {
    fn next(&self) -> Option<BackendGuard> {
        let min_conns = self
            .backends
            .iter()
            .filter(|b| b.is_healthy())
            .map(Backend::num_connections)
            .min()?;

        let tied = self
            .backends
            .iter()
            .enumerate()
            .filter(|(_, b)| b.is_healthy() && b.num_connections() == min_conns)
            .map(|(i, _)| i)
            .collect::<Vec<_>>();

        let &idx = tied.choose(&mut rand::rng())?;

        Some(BackendGuard::tracking(
            self.backends[idx].addr(),
            self.backends[idx].connection_counter(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{localhost_addr, make_healthy_backends};
    use anyhow::Context;
    use std::sync::atomic::Ordering;

    #[test]
    fn single_backend_always_returns_same_addr() -> Result<()> {
        let addr = localhost_addr(8001);
        let lc = LeastConnections::init(make_healthy_backends(&[8001]))?;

        for _ in 0..3 {
            assert_eq!(lc.next().context("expected backend")?.addr(), addr);
        }

        Ok(())
    }

    #[test]
    fn empty_backends_returns_err() {
        assert!(LeastConnections::init(make_healthy_backends(&[])).is_err());
    }

    #[test]
    fn skips_unhealthy_backends() -> Result<()> {
        let (a, b) = (localhost_addr(8001), localhost_addr(8002));
        let lc = LeastConnections::init(vec![Backend::healthy(a), Backend::unhealthy(b)].into())?;

        for _ in 0..3 {
            assert_eq!(lc.next().context("expected backend")?.addr(), a);
        }

        Ok(())
    }

    #[test]
    fn returns_none_when_all_unhealthy() -> Result<()> {
        let lc = LeastConnections::init(
            vec![
                Backend::unhealthy(localhost_addr(8001)),
                Backend::unhealthy(localhost_addr(8002)),
            ]
            .into(),
        )?;

        assert!(lc.next().is_none());
        assert!(lc.next().is_none());

        Ok(())
    }

    #[test]
    fn routes_to_backend_with_fewest_connections() -> Result<()> {
        let backends = make_healthy_backends(&[8000, 8001]);
        let lc = LeastConnections::init(Arc::clone(&backends))?;

        // Give 8000 two active connections so 8001 always wins
        backends[0]
            .connection_counter()
            .fetch_add(2, Ordering::Relaxed);

        for _ in 0..5 {
            // The guard drops at the end of each iteration, decrementing 8001 back to 0 while 8000
            // stays at 2
            assert_eq!(
                lc.next().context("expected backend")?.addr(),
                backends[1].addr()
            );
        }

        Ok(())
    }

    #[test]
    fn increments_count_on_next_and_decrements_on_drop() -> Result<()> {
        let backends = make_healthy_backends(&[8001]);
        let lc = LeastConnections::init(Arc::clone(&backends))?;

        assert_eq!(backends[0].num_connections(), 0);

        let guard = lc.next().context("expected backend")?;
        assert_eq!(backends[0].num_connections(), 1);

        drop(guard);
        assert_eq!(backends[0].num_connections(), 0);

        Ok(())
    }

    #[test]
    fn chooses_valid_backend_when_tied() -> Result<()> {
        let (a, b) = (localhost_addr(8001), localhost_addr(8002));
        let lc = LeastConnections::init(make_healthy_backends(&[8001, 8002]))?;

        for _ in 0..10 {
            let addr = lc.next().context("expected backend")?.addr();
            assert!(addr == a || addr == b);
        }

        Ok(())
    }
}
