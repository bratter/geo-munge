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
    Client { message: String },
}
