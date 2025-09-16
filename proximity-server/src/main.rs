use anyhow::Result;
use clap::Parser;
use proximity_ipc::{set_ctrlc_handler, Context};
use proximity_server::{args::Args, config::Config};
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    // Enable tracing
    // TODO: Configure better - have option to pass a level or something on the CLI
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    // Set up graceful ctrl-c handling
    let running = set_ctrlc_handler()?;

    // TODO: Parse here? Need to fix the arguments, add socket handling at least
    // The args can also basically take a reset option too
    let args = Args::parse();

    // Get the right socket name
    // TODO: Improve socket handling - add manual option in CLI interface
    let socket;
    #[cfg(unix)]
    {
        socket = proximity_ipc::DEFAULT_UNIX_SOCKET_NAME;
    }
    #[cfg(windows)]
    {
        socket = proximity_ipc::DEFAULT_TCP_SOCKET_ADDR;
    }

    // TODO: We will want to be able to put the server in bench mode, that will likely need to inject at least ready
    // into context
    let config = Config::default().set_socket_name(socket);

    let context = Context {
        config,
        running,
        ready: None,
    };

    proximity_server::run(context)
}
