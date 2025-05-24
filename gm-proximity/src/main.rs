//! GM-Proximity
//!
//! Proximity client-server binary.

mod args;
mod client;
mod connection;
mod ctrlc;
mod message;
mod server;

use anyhow::Result;
use clap::Parser;

use crate::args::{Args, ClientCommandWrapper, Command};

#[cfg(unix)]
const UNIX_SOCKET_NAME: &str = "/tmp/gm-proximity";
#[cfg(windows)]
const WINDOWS_PIPE_NAME: &str = r"\\.\pipe\gm_proximity";

/// The maximum connection pool size for client connections - required to ensure that the SERVER token stays separated
pub const MAX_CONNECTIONS: usize = 8;

fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Command::Server => crate::server::run(crate::server::Config::default()),
        Command::Client(ClientCommandWrapper { command }) => {
            crate::client::run(command, crate::client::Config::default())
        }
    }
}
