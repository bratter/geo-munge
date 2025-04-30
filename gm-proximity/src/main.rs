//! GM-Proximity
//!
//! Proximity client-server binary.

mod args;
mod client;
mod message;
mod server;

use anyhow::Result;
use clap::Parser;

use crate::args::{Args, Command};

const SOCKET_NAME: &str = "@gm_proximity_socket";

fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Command::Server => crate::server::run(),
        Command::Client { message } => crate::client::run(&message),
    }
}
