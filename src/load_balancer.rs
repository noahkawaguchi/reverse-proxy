use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

/// A handle to a chosen backend. Decrements the backend's active connection count on drop if
/// connection tracking is enabled.
pub struct BackendGuard {
    addr: SocketAddr,
    counter: Option<Arc<AtomicUsize>>,
}

impl BackendGuard {
    pub const fn new(addr: SocketAddr) -> Self { Self { addr, counter: None } }

    /// Increments the backend's connection count and returns a guard that decrements it on drop.
    pub fn tracking(addr: SocketAddr, counter: Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Self { addr, counter: Some(counter) }
    }

    pub const fn addr(&self) -> SocketAddr { self.addr }
}

impl Drop for BackendGuard {
    fn drop(&mut self) {
        if let Some(c) = &self.counter {
            c.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

pub trait LoadBalancer: Send + Sync {
    /// Returns a guard for the next healthy backend, or `None` if all backends are unhealthy.
    fn next(&self) -> Option<BackendGuard>;
}
