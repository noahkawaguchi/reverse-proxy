use std::{
    net::SocketAddr,
    sync::atomic::{AtomicBool, Ordering},
};

/// A backend address with a healthy or unhealthy state.
pub struct Backend {
    addr: SocketAddr,
    healthy: AtomicBool,
}

impl Backend {
    /// Creates a new `Backend` with a status of healthy.
    pub const fn healthy(addr: SocketAddr) -> Self { Self { addr, healthy: AtomicBool::new(true) } }

    /// Creates a new `Backend` with a status of unhealthy.
    #[cfg(test)]
    pub const fn unhealthy(addr: SocketAddr) -> Self {
        Self { addr, healthy: AtomicBool::new(false) }
    }

    pub const fn addr(&self) -> SocketAddr { self.addr }

    /// Atomically retrieves the health status of the `Backend`.
    pub fn is_healthy(&self) -> bool { self.healthy.load(Ordering::Relaxed) }

    /// Atomically sets the health status of the `Backend`.
    pub fn set_health(&self, new_health: bool) {
        self.healthy.store(new_health, Ordering::Relaxed);
    }
}
