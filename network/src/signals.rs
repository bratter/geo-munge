use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use anyhow::Result;

/// Attempt to set a graceful Ctrl-c handler.
///
/// Returns a [`RunToken`] that indicates whether the application should attempt to shut down. An additional ctrl-c will
/// forcibly terminate.
///
/// The returned [`RunToken`] can also be used to signal shutdown manually.
pub fn set_ctrlc_handler() -> Result<RunToken> {
    let running = RunToken::new();
    let term_now = Arc::new(AtomicBool::new(false));
    let r = running.clone();

    ctrlc::set_handler(move || {
        // If we have already entered the handler once and are now back a second time, we want to perform a hard
        // termination. This might happen if one of the streams blocks for an extended period of time.
        // NOTE: `signal_hook` crate uses libc `_exit()` rather than `std::process::exit`, but don't think it is necessary
        // here, see: https://github.com/vorner/signal-hook/blob/master/src/low_level/mod.rs
        if term_now.load(Ordering::SeqCst) {
            tracing::warn!("Second ctrl-c detected, terminating...");
            std::process::exit(1);
        }
        // If we are not terminating immediately, then try to gracefully exit, but inform the handler that another
        // ctrl-c will terminate immediately.
        tracing::warn!("Ctrl-c detected, attempting graceful shutdown...");
        r.shutdown();
        term_now.store(true, Ordering::SeqCst);
    })?;

    Ok(running)
}

/// A simple token to indicate whether the application should be attempting to shutdown.
///
/// Wraps an [`AtomicBool`] and implements [`Clone`] so should be cloned and passed around, but abstracts some of the
/// boilerplate.
///
/// When the token is `false` it indicates that the application should attempt to gracefully exit. There are two ways to
/// make the comparison:
/// 1. Compare with a boolean to use the [`PartialEq`] impl: `token == true`
/// 2. Use the `is_running` method to get a boolean `token.is_running()`
///
/// Primarily used as the return value from the ctrl-c handler, but can also be used to shutdown manually by using the
/// `.shutdown()` method.
#[derive(Clone)]
pub struct RunToken(Arc<AtomicBool>);

impl RunToken {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }

    pub fn is_running(&self) -> bool {
        self == &true
    }

    pub fn shutdown(&self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl PartialEq<bool> for RunToken {
    fn eq(&self, other: &bool) -> bool {
        self.0.load(Ordering::SeqCst) == *other
    }
}
