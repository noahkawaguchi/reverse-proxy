use crate::backend::BackendGuard;

pub trait LoadBalancer: Send + Sync {
    /// Returns a guard for the next healthy backend, or `None` if all backends are unhealthy.
    fn next(&self) -> Option<BackendGuard>;
}
