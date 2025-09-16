use std::{borrow::Cow, time::Duration};

use network::server::IoLoopConfig;

/// Proximity server configuration.
pub struct Config {
    /// Configuration for the server io loop.
    pub io: IoLoopConfig,

    /// Timeout for when to check whether a shutdown has been triggered. Only use this when the work prevented by
    /// blocking is a shutdown check. The value can be high as manual shutdown is not performance critical.
    pub shutdown_timeout: Duration,

    /// The size of the request channel.
    ///
    /// Governs how many requests can be queued at a time, providing backpressure to the readers by not allowing
    /// progress when the queue is full - this prevents memory overconsumption by stacking up the size of the read
    /// channel when processing or sending is slow. Should be large enough to not inhibit processing speed.
    /// TODO: If batching into processing, this should be a multiple of the batch size
    pub request_capacity: usize,

    /// The size of the response channel.
    ///
    /// Governs how many responses can be queued from processing, providing backpressure to the calculation. If the
    /// calculations outpace writing out, this will prevent memory explosion. Generally this should be large enough to
    /// not get held up by draining to the write queue, as this only happens once each run through the loop.
    /// Note that this capacity is in addition to the capacity in each connection's write queue (which is unbounded but
    /// will turn off reading when too large).
    /// TODO: Should we drain the response channel in a couple of other places to help keep the size of this down? Maybe
    /// test when tuning io
    pub response_capacity: usize,
}

impl Config {
    pub fn set_socket_name(mut self, s: impl Into<Cow<'static, str>>) -> Self {
        #[cfg(unix)]
        {
            self.io.unix_socket_name = s.into();
        }

        #[cfg(windows)]
        {
            self.io.tcp_socket_addr = s.into();
        }

        self
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            io: IoLoopConfig::default(),
            shutdown_timeout: Duration::from_millis(500),
            request_capacity: 1024,
            response_capacity: 1024,
        }
    }
}
