//! Utilities for managing cross platform streaming logic.

#[cfg(unix)]
use std::borrow::Cow;
use std::{
    io::{Read, Write},
    net::SocketAddr,
};

use anyhow::Result;
#[cfg(unix)]
use mio::net::UnixListener;
use mio::{event::Source, net::TcpListener, Interest, Poll, Registry, Token};

#[cfg(unix)]
pub const DEFAULT_UNIX_SOCKET_NAME: &str = "/tmp/net_lib_socket";
#[cfg(not(unix))]
pub const DEFAULT_TCP_SOCKET_ADDR: &str = "127.0.0.1:6378";

pub enum SocketMode {
    #[cfg(unix)]
    // Locking the Cow lifetime to 'static to avoid the SocketMode having a lifetime that will throw an unused lifetime
    // error when compiling for windows. Seeing as we only use Cow to pass const &str, the lifetime isn't really
    // necessary. We could simply use a String, but the Cow is easy enough.
    Unix(Cow<'static, str>),
    Tcp(SocketAddr),
}

impl SocketMode {
    /// Create a [`SocketMode`] wrapper for a unix socket.
    ///
    /// We enforce that the socket's file is in `/tmp/`.
    #[cfg(unix)]
    pub fn unix(path: impl Into<Cow<'static, str>>) -> Result<Self> {
        use anyhow::bail;

        let cow: Cow<_> = path.into();
        if !cow.starts_with("/tmp/") {
            bail!("Unix socket path must start with /tmp/, got: {}", cow);
        }

        Ok(SocketMode::Unix(cow))
    }

    pub fn tcp(addr: impl AsRef<str>) -> Result<Self> {
        let addr_str = addr.as_ref();
        let socket_addr = addr_str
            .parse::<SocketAddr>()
            .map_err(|e| anyhow::anyhow!("Invalid socket address '{}': {}", addr_str, e))?;

        Ok(SocketMode::Tcp(socket_addr))
    }

    pub fn bind_listener_and_register(&self, poll: &Poll, token: Token) -> Result<Listener> {
        match self {
            #[cfg(unix)]
            SocketMode::Unix(path) => {
                // Remove the socket file before binding, ignoring errors (its fine if it doesn't exist)
                let _ = std::fs::remove_file(path.as_ref());
                let mut listener = UnixListener::bind(path.as_ref())?;
                poll.registry()
                    .register(&mut listener, token, Interest::READABLE)?;
                Ok(Listener::Unix(listener))
            }
            SocketMode::Tcp(addr) => {
                let mut listener = TcpListener::bind(addr.clone())?;
                poll.registry()
                    .register(&mut listener, token, Interest::READABLE)?;
                Ok(Listener::Tcp(listener))
            }
        }
    }
}

impl Default for SocketMode {
    fn default() -> Self {
        #[cfg(unix)]
        {
            Self::unix(DEFAULT_UNIX_SOCKET_NAME).expect("Valid default")
        }
        #[cfg(windows)]
        {
            Self::tcp(DEFAULT_TCP_SOCKET_ADDR).expect("Valid default")
        }
    }
}

impl std::fmt::Display for SocketMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(unix)]
            Self::Unix(str) => write!(f, "Socket name {}", str),
            Self::Tcp(str) => write!(f, "TCP socket {}", str),
        }
    }
}

pub enum Listener {
    #[cfg(unix)]
    Unix(UnixListener),
    Tcp(TcpListener),
}

impl Listener {
    pub fn accept(&self) -> std::io::Result<Stream> {
        match self {
            #[cfg(unix)]
            Listener::Unix(listener) => Ok(Stream::Unix(listener.accept()?.0)),
            Listener::Tcp(listener) => Ok(Stream::Tcp(listener.accept()?.0)),
        }
    }
}

pub enum Stream {
    #[cfg(unix)]
    Unix(mio::net::UnixStream),
    Tcp(mio::net::TcpStream),
}

impl Source for Stream {
    fn register(
        &mut self,
        registry: &Registry,
        token: Token,
        interests: Interest,
    ) -> std::io::Result<()> {
        match self {
            #[cfg(unix)]
            Stream::Unix(stream) => stream.register(registry, token, interests),
            Stream::Tcp(stream) => stream.register(registry, token, interests),
        }
    }

    fn reregister(
        &mut self,
        registry: &Registry,
        token: Token,
        interests: Interest,
    ) -> std::io::Result<()> {
        match self {
            #[cfg(unix)]
            Stream::Unix(stream) => stream.reregister(registry, token, interests),
            Stream::Tcp(stream) => stream.reregister(registry, token, interests),
        }
    }

    fn deregister(&mut self, registry: &Registry) -> std::io::Result<()> {
        match self {
            #[cfg(unix)]
            Stream::Unix(stream) => stream.deregister(registry),
            Stream::Tcp(stream) => stream.deregister(registry),
        }
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            #[cfg(unix)]
            Stream::Unix(stream) => stream.read(buf),
            Stream::Tcp(stream) => stream.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            #[cfg(unix)]
            Stream::Unix(stream) => stream.write(buf),
            Stream::Tcp(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            #[cfg(unix)]
            Stream::Unix(stream) => stream.flush(),
            Stream::Tcp(stream) => stream.flush(),
        }
    }
}
