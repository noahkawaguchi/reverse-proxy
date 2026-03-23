use anyhow::{Result, bail};
use serde::Deserialize;
use std::{
    net::SocketAddr,
    sync::atomic::{AtomicUsize, Ordering},
};

/// One or more backend addresses to be cycled through for round robin load balancing.
#[derive(Deserialize)]
#[serde(try_from = "Vec<SocketAddr>")]
pub struct RoundRobin {
    addrs: Vec<SocketAddr>,
    counter: AtomicUsize,
}

impl RoundRobin {
    pub fn next_addr(&self) -> SocketAddr {
        self.addrs[self.counter.fetch_add(1, Ordering::Relaxed) % self.addrs.len()]
    }
}

impl TryFrom<Vec<SocketAddr>> for RoundRobin {
    type Error = anyhow::Error;

    fn try_from(addrs: Vec<SocketAddr>) -> Result<Self, Self::Error> {
        if addrs.is_empty() {
            bail!("route must have at least one backend address");
        }

        Ok(Self { addrs, counter: AtomicUsize::new(0) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::localhost_addr;

    #[test]
    fn cycles_through_backends_in_order() -> Result<()> {
        let (a1, a2, a3) = (
            localhost_addr(8001),
            localhost_addr(8002),
            localhost_addr(8003),
        );

        let rr = RoundRobin::try_from(vec![a1, a2, a3])?;

        assert_eq!(rr.next_addr(), a1);
        assert_eq!(rr.next_addr(), a2);
        assert_eq!(rr.next_addr(), a3);
        assert_eq!(rr.next_addr(), a1);

        Ok(())
    }

    #[test]
    fn single_backend_always_returns_same_addr() -> Result<()> {
        let addr = localhost_addr(8001);
        let rr = RoundRobin::try_from(vec![addr])?;

        assert_eq!(rr.next_addr(), addr);
        assert_eq!(rr.next_addr(), addr);

        Ok(())
    }

    #[test]
    fn empty_addrs_returns_err() {
        assert!(RoundRobin::try_from(vec![]).is_err());
    }
}
