use std::time::Duration;

use anyhow::Result;
use crossbeam::channel::{self, Receiver, Sender};

use crate::{args::ClientCommand, message::prelude::*};

use super::{handlers, Tracker};

/// Client command handler. Translates client commands into requests.
pub struct CommandHandler {
    pub reponse_handler: ResponseHandler,
    request_tx: Sender<(u32, Request)>,
    tracker: Tracker,
    done_send: Sender<()>,
    next_req_id: u32,
}

impl CommandHandler {
    pub fn new(request_tx: Sender<(u32, Request)>, tracker: Tracker) -> (Self, Receiver<()>) {
        let (done_send, done_recv) = channel::bounded::<()>(1);
        let handler = Self {
            reponse_handler: ResponseHandler::None,
            request_tx,
            tracker,
            done_send,
            next_req_id: 0,
        };

        (handler, done_recv)
    }

    pub fn handle(&mut self, req: ClientCommand) -> Result<()> {
        let res = match req {
            ClientCommand::Stats => {
                self.send(Request::Stats)?;
                Ok(())
            }
            ClientCommand::Reset(r) => handlers::reset(self, r),
            ClientCommand::Load { file } => handlers::load(self, file),
            ClientCommand::Get(get_args) => handlers::get(self, get_args),
            ClientCommand::Delete(delete_args) => handlers::delete(self, delete_args),
            ClientCommand::Knn(knn_args) => handlers::knn(self, knn_args),
            ClientCommand::Window(window_args) => handlers::window(self, window_args),
            ClientCommand::Repl => handlers::repl(self),
            ClientCommand::Bench(bench_args) => handlers::bench(self, bench_args),
        };

        // As this is a oneshot and shouldn't be called anywhere else, we don't care about the result
        tracing::info!("Request done, sending done notification");
        _ = self.done_send.try_send(());

        res
    }

    /// Send a request for dispatch.
    ///
    /// Will block until the request channel has capacity.
    pub fn send(&mut self, req: Request) -> Result<()> {
        // Get an id and bump to the next one
        let id = self.next_req_id;
        self.next_req_id += 1;

        // Track the request before sending so we know when we have received responses
        // TODO: Unwind the tracking if this fails?
        let _ = self.tracker.track(id, &req, self.reponse_handler.clone());
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
// TODO: Currently just cloning these to avoid holding the lock for too long. If they get big, consider not bothering
// (responses are single threaded anyway, and blocking requests is unlikely to be an issue) use RWLock (unlikely to
// help), wrapping the TrackerData in an Arc and using that to clone, or something else.
// TODO: We can't clone if we want to mutate anyway
#[derive(Default, Clone)]
pub enum ResponseHandler {
    #[default]
    None,
    Repl(Sender<()>),
    Bench(Option<Duration>),
}

impl ResponseHandler {
    // TODO: Better flow with duration
    pub fn handle(&self, id: u32, done: Option<Duration>, res: &Response) {
        // TODO: Remove when stable or a non-clone solution found
        // Checking that the response handler doesn't get too big to clone
        debug_assert!(std::mem::size_of::<ResponseHandler>() <= 32);

        match self {
            // TODO: As a placeholder, print responses until we have better handling
            ResponseHandler::None => Self::print(id, done, res),
            // TODO: Perhaps some more sophisticated repl handling here
            ResponseHandler::Repl(sender) => {
                // Signal that the request is complete when the last response has been recieved
                Self::print(id, done, res);
                if done.is_some() {
                    sender.send(()).expect("channel disconnected");
                }
            }
            ResponseHandler::Bench(receive_delay) => {
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
        // TODO: This is very inefficient, assume it will be removed... if not fix
        let mut prefix = format!("[req {}", id);
        if let Some(duration) = duration {
            prefix.push_str(&format!("; {}ms", duration.as_millis()));
        }
        prefix.push_str("]");

        match res {
            Response::Success(Some(msg)) => eprintln!("{} {}", prefix, msg),
            Response::Success(None) => eprintln!("{} success", prefix),
            // TODO: When handling done should cross-check number of responses
            Response::Done(n) => eprintln!("{} done with {} responses", prefix, n),
            Response::Stats(n) => eprintln!(
                "{} QT size={}; key: {:?}; bytes sent={}; bytes recv={}",
                prefix, n.qt_size, n.key_mode, n.bytes_sent, n.bytes_recv
            ),
            Response::ResultCounts { success, fail } => {
                eprintln!("{} succeed {}, failed {}", prefix, success, fail)
            }
            Response::BasicResults(results) => {
                for result in results {
                    match result {
                        Ok(basic_result) => {
                            let content_json = format_content(&basic_result.content);
                            println!("{},{}", basic_result.id, content_json);
                        }
                        Err(err) => eprintln!("Error: {}", err),
                    }
                }
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
                        Err(err) => eprintln!("Error: {}", err),
                    }
                }
            }
            Response::Error(msg) => eprintln!("{} error: {}", prefix, msg),
            Response::Bench(_) => unreachable!(),
        }
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
