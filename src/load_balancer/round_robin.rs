use crate::{
    backend::{Backend, BackendGuard},
    load_balancer::LoadBalancer,
};
use anyhow::{Result, bail};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

/// One or more backends to be cycled through for round robin load balancing.
/// Skips backends whose `healthy` flag is `false`.
#[cfg_attr(test, derive(Debug))]
pub struct RoundRobin {
    backends: Vec<Arc<Backend>>,
    counter: AtomicUsize,
}

impl RoundRobin {
    pub fn init(backends: Vec<Arc<Backend>>) -> Result<Self> {
        if backends.is_empty() {
            bail!("route must have at least one backend address");
        }

        Ok(Self { backends, counter: AtomicUsize::new(0) })
    }
}

impl LoadBalancer for RoundRobin {
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "Wrapping desired for round robin counter"
    )]
    fn next(&self) -> Option<BackendGuard> {
        let n = self.backends.len();
        let start = self.counter.fetch_add(1, Ordering::Relaxed) % n;
        let mut i = start;

        while !self.backends.get(i)?.is_healthy() {
            i = self.counter.fetch_add(1, Ordering::Relaxed) % n;
            if i == start {
                return None;
            }
        }

        Some(Arc::clone(self.backends.get(i).as_ref()?).acquire())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{localhost_addr, make_healthy_backends};
    use std::assert_matches;

    #[test]
    fn cycles_through_backends_in_order() -> Result<()> {
        let (a1, a2, a3) = (
            localhost_addr(8001),
            localhost_addr(8002),
            localhost_addr(8003),
        );

        let rr = RoundRobin::init(make_healthy_backends([8001, 8002, 8003]).to_vec())?;

        assert_eq!(rr.next().map(|g| g.addr()), Some(a1));
        assert_eq!(rr.next().map(|g| g.addr()), Some(a2));
        assert_eq!(rr.next().map(|g| g.addr()), Some(a3));
        assert_eq!(rr.next().map(|g| g.addr()), Some(a1));

        Ok(())
    }

    #[test]
    fn single_backend_always_returns_same_addr() -> Result<()> {
        let addr = localhost_addr(8001);
        let rr = RoundRobin::init(make_healthy_backends([8001]).to_vec())?;

        assert_eq!(rr.next().map(|g| g.addr()), Some(addr));
        assert_eq!(rr.next().map(|g| g.addr()), Some(addr));
        assert_eq!(rr.next().map(|g| g.addr()), Some(addr));

        Ok(())
    }

    #[test]
    fn empty_backends_returns_err() {
        assert_matches!(RoundRobin::init(make_healthy_backends([]).to_vec()), Err(_));
    }

    #[test]
    fn skips_unhealthy_backends() -> Result<()> {
        let (a1, a2, a3) = (
            localhost_addr(8001),
            localhost_addr(8002),
            localhost_addr(8003),
        );

        let rr = RoundRobin::init(vec![
            Arc::new(Backend::healthy(a1)),
            Arc::new(Backend::unhealthy(a2)),
            Arc::new(Backend::healthy(a3)),
        ])?;

        // Should alternate between a1 and a3 with a2 unhealthy
        assert_eq!(rr.next().map(|g| g.addr()), Some(a1));
        assert_eq!(rr.next().map(|g| g.addr()), Some(a3));
        assert_eq!(rr.next().map(|g| g.addr()), Some(a1));
        assert_eq!(rr.next().map(|g| g.addr()), Some(a3));

        Ok(())
    }

    #[test]
    fn returns_none_when_all_unhealthy() -> Result<()> {
        let rr = RoundRobin::init(vec![
            Arc::new(Backend::unhealthy(localhost_addr(8001))),
            Arc::new(Backend::unhealthy(localhost_addr(8002))),
        ])?;

        assert!(rr.next().is_none());
        assert!(rr.next().is_none());

        Ok(())
    }
}
