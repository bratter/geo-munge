use std::{
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use anyhow::Result;
use crossbeam::channel::{self, Receiver, Sender};

use crate::{args::ClientCommand, message::prelude::*};

use super::{handlers, Tracker};

/// Client command handler. Translates client commands into requests.
pub struct CommandHandler {
    pub response_handler: Arc<Mutex<ResponseHandler>>,
    request_tx: Sender<(u32, Request)>,
    tracker: Tracker,
    done_send: Sender<()>,
    next_req_id: AtomicU32,
}

impl CommandHandler {
    pub fn new(request_tx: Sender<(u32, Request)>, tracker: Tracker) -> (Self, Receiver<()>) {
        let (done_send, done_recv) = channel::bounded::<()>(1);
        let handler = Self {
            response_handler: Arc::new(Mutex::new(ResponseHandler::default())),
            request_tx,
            tracker,
            done_send,
            next_req_id: AtomicU32::new(0),
        };

        (handler, done_recv)
    }

    /// Handle the incoming request.
    ///
    /// This handler is a "one and done" so handle operates like a build.
    ///
    /// Because the client only runs a single command when invoked, the handle function here can take ownership of the
    /// CommandHandler. This affords us the flexibility to spawn threads in the handlers.
    pub fn handle(self, req: ClientCommand) -> Result<()> {
        let handler = Arc::new(self);

        let res = match req {
            ClientCommand::Stats => {
                handler.send(Request::Stats)?;
                Ok(())
            }
            ClientCommand::Reset(r) => handlers::reset(&handler, r),
            ClientCommand::Load { file } => handlers::load(&handler, file),
            ClientCommand::Get(get_args) => handlers::get(&handler, get_args),
            ClientCommand::Delete(delete_args) => handlers::delete(&handler, delete_args),
            ClientCommand::Knn(knn_args) => handlers::knn(&handler, knn_args),
            ClientCommand::Window(window_args) => handlers::window(&handler, window_args),
            ClientCommand::Repl => handlers::repl(Arc::clone(&handler)),
            ClientCommand::Bench(bench_args) => handlers::bench(Arc::clone(&handler), bench_args),
        };

        // As this is a oneshot and shouldn't be called anywhere else, we don't care about the result
        tracing::info!("Request done, sending done notification");
        _ = handler.done_send.try_send(());

        res
    }

    /// Send a request for dispatch.
    ///
    /// Will block until the request channel has capacity.
    #[tracing::instrument(skip_all)]
    pub fn send(&self, req: Request) -> Result<()> {
        tracing::debug!("Sent request: {:?}", req);

        // Get an id and bump to the next one
        // We are just using this as an id so we can safely use Relaxed ordering here
        let id = self.next_req_id.fetch_add(1, Ordering::Relaxed);
        let response_handler = self.response_handler.lock().expect("Lock poisoned").clone();

        // Track the request before sending so we know when we have received responses
        // TODO: Unwind the tracking if this fails?
        let _ = self.tracker.track(id, &req, response_handler);
        self.request_tx.send((id, req))?;

        Ok(())
    }

    /// Report the number of outstanding requests in the tracker.
    pub fn outstanding(&self) -> usize {
        self.tracker.outstanding()
    }
}

// TODO: These can be things like print or file output that handle multiple request types, or ones that are specific
// to the request
#[derive(Default, Clone)]
pub enum ResponseHandler {
    #[default]
    Print,
    Repl(Sender<()>),
    Bench(Option<Duration>),
}

impl ResponseHandler {
    // TODO: Better flow with duration
    #[tracing::instrument(skip_all)]
    pub fn handle(&self, id: u32, done: Option<Duration>, res: &Response) {
        // Checking that the response handler doesn't get too big to clone
        // If this gets thrown, consider converting to Arc
        debug_assert!(std::mem::size_of::<Self>() <= 64);

        tracing::debug!("Handling response: {:?}", res);

        match self {
            // TODO: As a placeholder, print responses until we have better handling - would ideally also allow
            // outputting to a file, but this is basically the same as print but with an outfile - it should do the same
            // thing as redirecting stdout, so maybe could just overload print with an optional filename
            Self::Print => Self::print(id, done, res),
            Self::Repl(sender) => {
                // Signal that the request is complete when the last response has been recieved
                Self::print(id, done, res);
                if done.is_some() {
                    sender.send(()).expect("channel disconnected");
                }
            }
            Self::Bench(receive_delay) => {
                match res {
                    // The response just contains empty data that we ignore, but we delay to simulate io lag
                    Response::Bench(_) => {
                        if let Some(t) = *receive_delay {
                            std::thread::sleep(t);
                        }
                    }
                    Response::Done(_) => {}
                    _ => unreachable!(),
                }
            }
        }
    }

    // TODO: Temporary print function - remove or refactor when handling improves
    // Print will likely remain the main output interface, but we could consider direct push to file or something else
    fn print(id: u32, duration: Option<Duration>, res: &Response) {
        use std::io::{stderr, Write};

        let mut stderr = stderr().lock();

        // Write prefix
        let _ = write!(stderr, "[req {}", id);
        if let Some(duration) = duration {
            let _ = write!(stderr, "; {}ms", duration.as_millis());
        }
        let _ = write!(stderr, "] ");

        // Ignore write errors like eprintln! does
        let _ = match res {
            Response::Success(Some(msg)) => writeln!(stderr, "{}", msg),
            Response::Success(None) => writeln!(stderr, "success"),
            // TODO: When handling done should cross-check number of responses
            Response::Done(n) => writeln!(stderr, "done with {} responses", n),
            Response::Stats(n) => writeln!(
                stderr,
                "QT size={}; key: {:?}; bytes sent={}; bytes recv={}",
                n.qt_size, n.key_mode, n.bytes_sent, n.bytes_recv
            ),
            Response::ResultCounts { success, fail } => {
                writeln!(stderr, "succeed {}, failed {}", success, fail)
            }
            Response::BasicResults(results) => {
                for result in results {
                    match result {
                        Ok(basic_result) => {
                            let content_json = format_content(&basic_result.content);
                            println!("{},{}", basic_result.id, content_json);
                        }
                        Err(err) => {
                            let _ = writeln!(stderr, "Error: {}", err);
                        }
                    }
                }
                Ok(())
            }
            Response::ProximityResults(results) => {
                for result in results {
                    match result {
                        Ok(proximity_result) => {
                            let content_json = format_content(&proximity_result.content);
                            println!(
                                "{},{},{},{}",
                                proximity_result.input_index,
                                proximity_result.id,
                                proximity_result.distance,
                                content_json
                            );
                        }
                        Err(err) => {
                            let _ = writeln!(stderr, "Error: {}", err);
                        }
                    }
                }
                Ok(())
            }
            Response::Error(msg) => writeln!(stderr, "error: {}", msg),
            Response::Bench(_) => unreachable!(),
        };
    }
}

fn format_content(content: &ContentType) -> String {
    let json_str = match content {
        ContentType::FullFeature(feature) => feature.to_string(),
        ContentType::GeometryOnly(geometry) => geometry.to_string(),
        ContentType::PropertiesOnly(json_value) => json_value.to_string(),
        ContentType::None => return String::new(),
    };

    // Ensure that the json strings are properly escaped in csv
    // TODO: Is this really what we want? Do we at least want some form of possible formatting options
    format!("\"{}\"", json_str.replace('"', "\"\""))
}
