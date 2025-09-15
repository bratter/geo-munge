use std::{borrow::Cow, time::Duration};

use anyhow::{anyhow, Result};
use network::{
    client::{run_io_loop, IoLoopConfig},
    connection::Traffic,
};
use proximity_ipc::channel::{ClientChannels, RecvTimeoutError};

use super::{CommandHandler, Tracker};
use crate::{args::ClientCommand, Context};

pub struct Config {
    /// Configuration for the client io loop.
    pub io: IoLoopConfig,

    /// Timeout for when to check whether a shutdown has been triggered. Only use this when the work prevented by
    /// blocking is a shutdown check. The value can be high as manual shutdown is not performance critical.
    pub shutdown_timeout: Duration,

    /// Size of the request channel.
    ///
    /// As this only writes out when writing is available, this capacity will backpressure any upstream io that relies
    /// on sending on this channel.
    pub request_capacity: usize,

    /// Size of the response channel.
    ///
    /// As this channel will only be drawn from when the client can process it, it will backpressure the server.
    pub response_capacity: usize,
}

// TODO: Tune the values of these config items
impl Default for Config {
    fn default() -> Self {
        let response_capacity = 1024;

        Self {
            io: IoLoopConfig::with_reenable_limit(response_capacity * 3 / 4),
            shutdown_timeout: Duration::from_millis(500),
            request_capacity: 1024,
            response_capacity,
        }
    }
}

impl Config {
    pub fn set_socket_name(mut self, s: impl Into<Cow<'static, str>>) -> Self {
        #[cfg(unix)]
        {
            self.io.unix_socket_name = s.into();
        }

        #[cfg(windows)]
        {
            self.io.tcp_socket_addr = s.into()
        }

        self
    }
}

pub fn run(cmd: ClientCommand, context: Context<Config>) -> Result<()> {
    let run_span = tracing::error_span!("client");
    let _enter = run_span.enter();

    // Set up message channels and instrumentation
    let config: &_ = Box::leak(Box::new(context.config));
    let channels = ClientChannels::new(config.request_capacity, config.response_capacity);
    let traffic: &_ = Box::leak(Box::new(Traffic::default()));
    let mut tracker = Tracker::default();
    let (cmd_handler, done) = CommandHandler::new(channels.request_tx, tracker.clone());

    let r = context.running.clone();
    let io_run_span = run_span.clone();
    let io_handle = std::thread::spawn(move || {
        let _enter = io_run_span.enter();
        match run_io_loop(
            r.clone(),
            &config.io,
            traffic,
            channels.request_rx,
            channels.response_tx,
        ) {
            Ok(_) => tracing::trace!("Client IO loop exit success"),
            Err(err) => tracing::error!("Client IO loop exit error: {}", err),
        }
        r.shutdown();
    });

    // Shift request handling onto its own thread. If this completes fast or is highly blocking (like an interactive
    // terminal) this thread will use minimal resources. If lots of disk/std io is required, it will run in the
    // background while still allowing response handling.
    let handle_run_span = run_span.clone();
    let cmd_handle = std::thread::spawn(move || {
        let _enter = handle_run_span.enter();
        match cmd_handler.handle(cmd) {
            Ok(_) => tracing::trace!("Client command handler exit success"),
            Err(err) => tracing::error!("Client command handler exit error: {}", err),
        }
    });

    // Run the response handling in the main thread using an additional flag to track whether the request thread has
    // finished - needs to be its own flag as the channel will report done only once. When done requesting, we know that
    // all reuqest ids have been entered in the tracker, so can wait until these are all cleared before exiting the
    // response loop. Without waiting for done requesting, there may just be no outstanding responses so this would exit
    // spuriously.
    let mut done_requesting = false;
    while context.running.is_running() {
        match channels.response_rx.recv_timeout(config.shutdown_timeout) {
            Ok((msg_token, res)) => tracker.handle(msg_token.msg(), res)?,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        };

        // Toggle the done flag the first time a message comes through on the channel
        if !done_requesting && done.try_recv().is_ok() {
            tracing::trace!("Setting done flag in response loop");
            done_requesting = true;
        }
        if done_requesting && tracker.outstanding() == 0 {
            break;
        }
    }

    // If we break out the the response loop, we must shut down
    context.running.shutdown();

    cmd_handle
        .join()
        .map_err(|_| anyhow!("Cmd handler thread join failed"))?;
    io_handle
        .join()
        .map_err(|_| anyhow!("IO thread join failed"))?;

    tracing::info!(
        "Client done; bytes sent={}; bytes recv={}",
        traffic.sent(),
        traffic.recv()
    );
    Ok(())
}
