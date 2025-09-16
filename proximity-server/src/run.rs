//! Server handler for GM-Proximity.

use std::sync::Arc;

use anyhow::Result;
use arc_swap::ArcSwap;
use crossbeam::channel::RecvTimeoutError;
use network::{connection::Traffic, server::run_io_loop};
use proximity_ipc::{channel::ServerChannels, Context};

use crate::config::Config;

use super::{geo::GeoStore, handler::Handler};

pub fn run(context: Context<Config>) -> Result<()> {
    let run_span = tracing::error_span!("server");
    let _enter = run_span.enter();

    let running = context.running;
    let config: &_ = Box::leak(Box::new(context.config));
    let traffic: &_ = Box::leak(Box::new(Traffic::default()));
    let server_channels = ServerChannels::new(config.request_capacity, config.response_capacity);

    // Start the IO loop
    let io_run_span = run_span.clone();
    let r = running.clone();
    let io_handle = std::thread::spawn(move || {
        let _enter = io_run_span.enter();
        match run_io_loop(
            r.clone(),
            &context.ready,
            &config.io,
            traffic,
            server_channels.request_tx,
            server_channels.response_rx,
        ) {
            Ok(_) => tracing::trace!("Server IO loop exited success"),
            Err(err) => tracing::error!("Server IO loop exit error: {}", err),
        };
        r.shutdown();
    });

    #[cfg(unix)]
    let listen_on = &config.io.unix_socket_name;
    #[cfg(windows)]
    let listen_on = &config.io.tcp_socket_addr;
    tracing::info!("Geo Munge Proximity server listening on: {}", listen_on);

    // Initialize the GeoStore and start the main processing loop
    // On the main thread we block on listening for messages on the request channel with a timeout to capture the
    // graceful shutdown - this timeout can be relatively long as the shutdown is not time-critical
    // Note that the handle function takes the channel rather than just returning the response as the server may choose
    // to chunk responses
    // TODO: Initializing with the default GeoStore options. This should be considered and aligned with bounding box and
    // key mode before finalizing (esp. given key mode is stored in the handler)
    let geo_store = ArcSwap::from(Arc::new(GeoStore::default()));
    let handler = Handler::new(geo_store, server_channels.response_tx);

    // TODO: Add parallelism back with better threading mechanism, note need to keep handler lightweight and clonable
    // TODO: Improve and instrument this loop - should the handler be cloned? Should we ignore channel shutdown?
    while running.is_running() {
        match server_channels
            .request_rx
            .recv_timeout(config.shutdown_timeout)
        {
            Ok(req) => handler.handle(req, traffic),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                if running.is_running() {
                    tracing::error!("Request channel disconnected, shutting down");
                    running.shutdown();
                    break;
                }
            }
        }
    }

    io_handle.join().expect("Couldn't join io handle");
    tracing::info!("Geo Munge Proximity server shut down successfully");
    Ok(())
}
