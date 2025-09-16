mod args;
mod run;

use anyhow::Result;
use clap::Parser;
use proximity_ipc::set_ctrlc_handler;
use tracing_subscriber::EnvFilter;

use args::Args;

fn main() -> Result<()> {
    // Enable tracing
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    // Set up graceful ctrl-c handling
    let running = set_ctrlc_handler()?;

    let args = Args::parse();

    run::run(args, running)
}

