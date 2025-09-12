use std::sync::atomic::{AtomicUsize, Ordering};

/// Global tracker for traffic on a [`ConnectionPool`].
///
/// Uses [`AtomicUsize`] internally to enable use through shared references.
#[derive(Default)]
pub struct Traffic {
    sent: AtomicUsize,
    recv: AtomicUsize,
}

impl Traffic {
    pub fn sent(&self) -> usize {
        self.sent.load(Ordering::Relaxed)
    }

    pub fn recv(&self) -> usize {
        self.recv.load(Ordering::Relaxed)
    }

    /// Add sent traffic in bytes.
    ///
    /// Uses [`AtomicUsize::fetch_add`], so returns the previous value.
    pub fn add_send(&self, bytes: usize) -> usize {
        self.sent.fetch_add(bytes, Ordering::Relaxed)
    }

    /// Add received traffic in bytes.
    ///
    /// Uses [`AtomicUsize::fetch_add`], so returns the previous value.
    pub fn add_recv(&self, bytes: usize) -> usize {
        self.recv.fetch_add(bytes, Ordering::Relaxed)
    }
}
