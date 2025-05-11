use clap::{Parser, Subcommand};

/// Command line client and server for proximity-based geospatial operations.
#[derive(Debug, Parser)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Server,
    Client(ClientCommandWrapper),
}

#[derive(Debug, Parser)]
pub struct ClientCommandWrapper {
    #[command(subcommand)]
    pub command: ClientCommand,
}

#[derive(Debug, Subcommand)]
pub enum ClientCommand {
    Stats,
    Msg { message: String },
}
