use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

/// Runtime metrics for a `SharedBody`.
///
/// A lightweight, read-only view exposing the number of active clones.
/// Obtain a `Stats` handle via `SharedBody::stats()`. Values use relaxed
/// atomics and are intended for diagnostics.
#[cfg_attr(docsrs, doc(cfg(feature = "stats")))]
#[derive(Debug, Clone)]
pub struct Stats {
    active_clones: Arc<AtomicU64>,
}

impl Stats {
    // Create a new, empty stats instance.
    pub(crate) fn new() -> Self {
        Self {
            active_clones: Arc::new(AtomicU64::new(1)),
        }
    }

    pub(crate) fn increment(&self) {
        self.active_clones.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn decrement(&self) {
        self.active_clones.fetch_sub(1, Ordering::Relaxed);
    }

    /// Returns the number of active clones for the associated `SharedBody`.
    ///
    /// The count includes the instance this `Stats` handle was obtained from,
    /// so a value of `1` indicates there are no additional clones.
    pub fn active_clones(&self) -> u64 {
        self.active_clones.load(Ordering::Relaxed)
    }
}
