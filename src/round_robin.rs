use crate::{
    backend::Backend,
    load_balancer::{BackendGuard, LoadBalancer},
};
use anyhow::{Result, bail};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

/// One or more backends to be cycled through for round robin load balancing.
/// Skips backends whose `healthy` flag is `false`.
pub struct RoundRobin {
    backends: Arc<[Backend]>,
    counter: AtomicUsize,
}

impl RoundRobin {
    pub fn init(backends: Arc<[Backend]>) -> Result<Self> {
        if backends.is_empty() {
            bail!("route must have at least one backend address");
        }

        Ok(Self { backends, counter: AtomicUsize::new(0) })
    }
}

impl LoadBalancer for RoundRobin {
    fn next(&self) -> Option<BackendGuard> {
        let n = self.backends.len();
        let start = self.counter.fetch_add(1, Ordering::Relaxed) % n;
        let mut i = start;

        while !self.backends[i].is_healthy() {
            i = self.counter.fetch_add(1, Ordering::Relaxed) % n;
            if i == start {
                return None;
            }
        }

        Some(BackendGuard::new(self.backends[i].addr()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{localhost_addr, make_healthy_backends};

    #[test]
    fn cycles_through_backends_in_order() -> Result<()> {
        let (a1, a2, a3) = (
            localhost_addr(8001),
            localhost_addr(8002),
            localhost_addr(8003),
        );

        let rr = RoundRobin::init(make_healthy_backends(&[8001, 8002, 8003]))?;

        assert_eq!(rr.next().map(|g| g.addr()), Some(a1));
        assert_eq!(rr.next().map(|g| g.addr()), Some(a2));
        assert_eq!(rr.next().map(|g| g.addr()), Some(a3));
        assert_eq!(rr.next().map(|g| g.addr()), Some(a1));

        Ok(())
    }

    #[test]
    fn single_backend_always_returns_same_addr() -> Result<()> {
        let addr = localhost_addr(8001);
        let rr = RoundRobin::init(make_healthy_backends(&[8001]))?;

        assert_eq!(rr.next().map(|g| g.addr()), Some(addr));
        assert_eq!(rr.next().map(|g| g.addr()), Some(addr));
        assert_eq!(rr.next().map(|g| g.addr()), Some(addr));

        Ok(())
    }

    #[test]
    fn empty_backends_returns_err() {
        assert!(RoundRobin::init(make_healthy_backends(&[])).is_err());
    }

    #[test]
    fn skips_unhealthy_backends() -> Result<()> {
        let (a1, a2, a3) = (
            localhost_addr(8001),
            localhost_addr(8002),
            localhost_addr(8003),
        );

        let rr = RoundRobin::init(
            vec![
                Backend::healthy(a1),
                Backend::unhealthy(a2),
                Backend::healthy(a3),
            ]
            .into(),
        )?;

        // Should alternate between a1 and a3 with a2 unhealthy
        assert_eq!(rr.next().map(|g| g.addr()), Some(a1));
        assert_eq!(rr.next().map(|g| g.addr()), Some(a3));
        assert_eq!(rr.next().map(|g| g.addr()), Some(a1));
        assert_eq!(rr.next().map(|g| g.addr()), Some(a3));

        Ok(())
    }

    #[test]
    fn returns_none_when_all_unhealthy() -> Result<()> {
        let rr = RoundRobin::init(
            vec![
                Backend::unhealthy(localhost_addr(8001)),
                Backend::unhealthy(localhost_addr(8002)),
            ]
            .into(),
        )?;

        assert!(rr.next().is_none());
        assert!(rr.next().is_none());

        Ok(())
    }
}
