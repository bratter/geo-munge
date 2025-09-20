use anyhow::Result;
use clap::Parser;
use protocol::request::KeyMode;
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

    // Read CLI arguments and create config
    let args = Args::parse();
    let mut config = Config::default();
    config.initial_bbox = args.initial_config.bbox.unwrap_or_default();
    config.initial_key_mode = if let Some(ptr) = args.initial_config.byte_keys {
        KeyMode::CustomBytes(ptr)
    } else if args.initial_config.provided_keys {
        KeyMode::ProvidedNumeric
    } else {
        KeyMode::AutoIncrement
    };

    // Configure socket based on args
    #[cfg(unix)]
    {
        if args.tcp {
            // TCP mode on Unix
            match args.socket {
                Some(addr) => config.set_tcp_socket(addr)?,
                None => config.set_tcp_socket(proximity_ipc::DEFAULT_TCP_SOCKET_ADDR)?,
            }
        } else {
            // Unix socket mode (default on Unix)
            match args.socket {
                Some(path) => config.set_unix_socket(path)?,
                None => config.set_unix_socket(proximity_ipc::DEFAULT_UNIX_SOCKET_NAME)?,
            }
        }
    }
    #[cfg(windows)]
    {
        // Windows always uses TCP
        match args.socket {
            Some(addr) => config.set_tcp_socket(addr)?,
            None => config.set_tcp_socket(proximity_ipc::DEFAULT_TCP_SOCKET_ADDR)?,
        }
    }

    let context = Context {
        config,
        running,
        ready: None,
    };

    proximity_server::run(context)
}
