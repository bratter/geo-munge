use std::{borrow::Cow, time::Duration};

use network::client::IoLoopConfig;

/// Proximity client configuration.
pub struct Config {
    /// Configuration for the client io loop.
    pub io: IoLoopConfig,

    /// Timeout for when to check whether a shutdown has been triggered. Only use this when the work prevented by
    /// blocking is a shutdown check. The value can be high as manual shutdown is not performance critical.
    pub shutdown_timeout: Duration,

    /// Size of the request channel.
    ///
    /// As this only writes out when writing is available, this capacity will backpressure any upstream io that relies
    /// on sending on this channel.
    pub request_capacity: usize,

    /// Size of the response channel.
    ///
    /// As this channel will only be drawn from when the client can process it, it will backpressure the server.
    pub response_capacity: usize,
}

// TODO: Tune the values of these config items
impl Default for Config {
    fn default() -> Self {
        let response_capacity = 1024;

        Self {
            io: IoLoopConfig::with_reenable_limit(response_capacity * 3 / 4),
            shutdown_timeout: Duration::from_millis(500),
            request_capacity: 1024,
            response_capacity,
        }
    }
}

impl Config {
    pub fn set_socket_name(mut self, s: impl Into<Cow<'static, str>>) -> Self {
        #[cfg(unix)]
        {
            self.io.unix_socket_name = s.into();
        }

        #[cfg(windows)]
        {
            self.io.tcp_socket_addr = s.into()
        }

        self
    }
}
