//! GM-Proximity
//!
//! Proximity client-server binary.

mod args;
mod bench;
mod client;
mod connection;
mod ctrlc;
mod input_io;
mod message;
mod server;

use anyhow::Result;
use clap::Parser;
use crossbeam::channel::Sender;
use ctrlc::{set_ctrlc_handler, RunToken};
use tracing_subscriber::EnvFilter;

use crate::args::{Args, ClientCommandWrapper, Command};

#[cfg(unix)]
const UNIX_SOCKET_NAME: &str = "/tmp/gm-proximity";
#[cfg(windows)]
const TCP_SOCKET_ADDR: &str = "127.0.0.1:6378";

/// The maximum connection pool size for client connections - required to ensure that the SERVER token stays separated
pub const MAX_CONNECTIONS: usize = 8;

pub struct Context<C> {
    pub config: C,
    pub ready: Option<Sender<()>>,
    pub running: RunToken,
}

fn main() -> Result<()> {
    // Enable tracing
    // TODO: Configure better
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    // Set up graceful ctrl-c handling
    let running = set_ctrlc_handler()?;

    let args = Args::parse();

    match args.command {
        Command::Server => crate::server::run(Context {
            config: crate::server::Config::default(),
            ready: None,
            running,
        }),
        Command::Client(ClientCommandWrapper { command }) => crate::client::run(
            command,
            Context {
                config: crate::client::Config::default(),
                ready: None,
                running,
            },
        ),
        Command::Bench(bench) => crate::bench::run(bench, running),
    }
}
