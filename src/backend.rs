use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

/// A handle to a backend. Decrements the backend's active connection count on drop.
pub struct BackendGuard {
    backend: Arc<Backend>,
}

impl BackendGuard {
    pub fn addr(&self) -> SocketAddr { self.backend.addr() }
}

impl Drop for BackendGuard {
    fn drop(&mut self) { self.backend.connections.fetch_sub(1, Ordering::Relaxed); }
}

/// A backend address with a healthy or unhealthy state and an active connection count.
#[cfg_attr(test, derive(Debug))]
pub struct Backend {
    addr: SocketAddr,
    healthy: AtomicBool,
    connections: AtomicUsize,
}

impl Backend {
    /// Creates a new `Backend` with a status of healthy and zero active connections.
    pub const fn healthy(addr: SocketAddr) -> Self {
        Self { addr, healthy: AtomicBool::new(true), connections: AtomicUsize::new(0) }
    }

    /// Creates a new `Backend` with a status of unhealthy and zero active connections.
    #[cfg(test)]
    pub const fn unhealthy(addr: SocketAddr) -> Self {
        Self { addr, healthy: AtomicBool::new(false), connections: AtomicUsize::new(0) }
    }

    /// Increments the active connection count and returns a guard that decrements it on drop.
    pub fn acquire(self: Arc<Self>) -> BackendGuard {
        self.connections.fetch_add(1, Ordering::Relaxed);
        BackendGuard { backend: self }
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
}
