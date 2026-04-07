use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

/// A backend address with a healthy or unhealthy state and an active connection count.
pub struct Backend {
    addr: SocketAddr,
    healthy: AtomicBool,
    connections: Arc<AtomicUsize>,
}

impl Backend {
    /// Creates a new `Backend` with a status of healthy and zero active connections.
    pub fn healthy(addr: SocketAddr) -> Self {
        Self { addr, healthy: AtomicBool::new(true), connections: Arc::new(AtomicUsize::new(0)) }
    }

    /// Creates a new `Backend` with a status of unhealthy and zero active connections.
    #[cfg(test)]
    pub fn unhealthy(addr: SocketAddr) -> Self {
        Self { addr, healthy: AtomicBool::new(false), connections: Arc::new(AtomicUsize::new(0)) }
    }

    pub const fn addr(&self) -> SocketAddr { self.addr }

    /// Atomically retrieves the health status of the `Backend`.
    pub fn is_healthy(&self) -> bool { self.healthy.load(Ordering::Relaxed) }

    /// Atomically sets the health status of the `Backend`.
    pub fn set_health(&self, new_health: bool) {
        self.healthy.store(new_health, Ordering::Relaxed);
    }

    /// Atomically retrieves the number of active connections to the `Backend`.
    pub fn num_connections(&self) -> usize { self.connections.load(Ordering::Relaxed) }

    pub fn connection_counter(&self) -> Arc<AtomicUsize> { Arc::clone(&self.connections) }
}
