//! Server handler for GM-Proximity.

use std::{sync::Arc, time::Duration};

use anyhow::Result;
use arc_swap::ArcSwap;
use crossbeam::channel::{self, RecvTimeoutError};
use network::{
    connection::{MsgToken, Traffic},
    server::{run_io_loop, IoLoopConfig},
};
use protocol::prelude::*;

use crate::Context;

use super::{geo_store::GeoStore, handle::Handler};

pub struct Config {
    /// Configuration for the server io loop.
    pub io: IoLoopConfig,

    /// Timeout for when to check whether a shutdown has been triggered. Only use this when the work prevented by
    /// blocking is a shutdown check. The value can be high as manual shutdown is not performance critical.
    pub shutdown_timeout: Duration,

    /// The size of the request channel.
    ///
    /// Governs how many requests can be queued at a time, providing backpressure to the readers by not allowing
    /// progress when the queue is full - this prevents memory overconsumption by stacking up the size of the read
    /// channel when processing or sending is slow. Should be large enough to not inhibit processing speed.
    /// TODO: If batching into processing, this should be a multiple of the batch size
    pub request_capacity: usize,

    /// The size of the response channel.
    ///
    /// Governs how many responses can be queued from processing, providing backpressure to the calculation. If the
    /// calculations outpace writing out, this will prevent memory explosion. Generally this should be large enough to
    /// not get held up by draining to the write queue, as this only happens once each run through the loop.
    /// Note that this capacity is in addition to the capacity in each connection's write queue (which is unbounded but
    /// will turn off reading when too large).
    /// TODO: Should we drain the response channel in a couple of other places to help keep the size of this down? Maybe
    /// test when tuning io
    pub response_capacity: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            io: IoLoopConfig::default(),
            shutdown_timeout: Duration::from_millis(500),
            request_capacity: 1024,
            response_capacity: 1024,
        }
    }
}

pub fn run(context: Context<Config>) -> Result<()> {
    let run_span = tracing::error_span!("server");
    let _enter = run_span.enter();

    let running = context.running;
    let config: &_ = Box::leak(Box::new(context.config));
    let traffic: &_ = Box::leak(Box::new(Traffic::default()));

    let (request_tx, request_rx) = channel::bounded::<(MsgToken, Request)>(config.request_capacity);
    let (response_tx, response_rx) =
        channel::bounded::<(MsgToken, Response)>(config.response_capacity);

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
            request_tx,
            response_rx,
        ) {
            Ok(_) => tracing::trace!("Server IO loop exited success"),
            Err(err) => tracing::error!("Server IO loop exit error: {}", err),
        };
        r.shutdown();
    });

    #[cfg(unix)]
    let listen_on = config.io.unix_socket_name;
    #[cfg(windows)]
    let listen_on = config.io.tcp_socket_addr;
    tracing::info!("Geo Munge Proximity server listening on: {}", listen_on);

    // Initialize the GeoStore and start the main processing loop
    // On the main thread we block on listening for messages on the request channel with a timeout to capture the
    // graceful shutdown - this timeout can be relatively long as the shutdown is not time-critical
    // Note that the handle function takes the channel rather than just returning the response as the server may choose
    // to chunk responses
    // TODO: Initializing with the default GeoStore options. This should be considered and aligned with bounding box and
    // key mode before finalizing (esp. given key mode is stored in the handler)
    let geo_store = ArcSwap::from(Arc::new(GeoStore::default()));
    let handler = Handler::new(geo_store, response_tx);

    // TODO: Add parallelism back with better threading mechanism, note need to keep handler lightweight and clonable
    // TODO: Improve and instrument this loop - should the handler be cloned? Should we ignore channel shutdown?
    while running.is_running() {
        match request_rx.recv_timeout(config.shutdown_timeout) {
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
