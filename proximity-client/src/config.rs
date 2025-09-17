#[cfg(unix)]
use std::borrow::Cow;
use std::time::Duration;

use anyhow::Result;
use network::{client::IoLoopConfig, stream::SocketMode};

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
    #[cfg(unix)]
    pub fn set_unix_socket(&mut self, path: impl Into<Cow<'static, str>>) -> Result<()> {
        self.io.socket_mode = SocketMode::unix(path)?;
        Ok(())
    }

    pub fn set_tcp_socket(&mut self, addr: impl AsRef<str>) -> Result<()> {
        self.io.socket_mode = SocketMode::tcp(addr)?;
        Ok(())
    }
}
