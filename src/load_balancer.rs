use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

/// A handle to a chosen backend address. For load balancers that track in-flight connections, the
/// counter is decremented when this guard is dropped.
pub struct BackendGuard {
    addr: SocketAddr,
    counter: Option<Arc<AtomicUsize>>,
}

impl BackendGuard {
    pub const fn new(addr: SocketAddr) -> Self { Self { addr, counter: None } }

    #[expect(dead_code, reason = "used by LeastConnections, not yet implemented")]
    pub const fn with_counter(counter: Arc<AtomicUsize>, addr: SocketAddr) -> Self {
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
