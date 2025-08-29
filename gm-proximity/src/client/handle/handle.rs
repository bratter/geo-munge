use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use anyhow::Result;
use crossbeam::channel::{self, Receiver, Sender};

use crate::{args::ClientCommand, message::prelude::*};

use super::{handlers, Res, Tracker};

/// Client command handler. Translates client commands into requests.
pub struct CommandHandler {
    request_tx: Sender<(u32, Request)>,
    tracker: Tracker,
    done_send: Sender<()>,
    next_req_id: AtomicU32,
}

impl CommandHandler {
    pub fn new(request_tx: Sender<(u32, Request)>, tracker: Tracker) -> (Self, Receiver<()>) {
        let (done_send, done_recv) = channel::bounded::<()>(1);
        let handler = Self {
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
    ///
    /// Due to the desire to use other handle functions in the repl (or nested in other handlers), we pass the correct
    /// response handler here rather than build it in the handler. This is a little messier, but means that when the
    /// handlers are used elsewhere, there is flexibility to pass the right ResponseHandler.
    pub fn handle(self, req: ClientCommand) -> Result<()> {
        let cmd = Arc::new(self);

        let response = match req {
            ClientCommand::Stats => {
                cmd.send(Request::Stats, &cmd.make_print_handler())?;
                Ok(())
            }
            ClientCommand::Reset(reset_args) => {
                handlers::reset(&cmd, &cmd.make_print_handler(), reset_args)
            }
            ClientCommand::Load { file: input_file } => {
                handlers::load(&cmd, &cmd.make_print_handler(), input_file)
            }
            ClientCommand::Get(get_args) => handlers::get(
                &cmd,
                &cmd.make_print_or_file_handler(&get_args.output)?,
                get_args,
            ),
            ClientCommand::Delete(delete_args) => {
                handlers::delete(&cmd, &cmd.make_print_handler(), delete_args)
            }
            ClientCommand::Knn(knn_args) => handlers::knn(
                &cmd,
                &cmd.make_print_or_file_handler(&knn_args.output)?,
                knn_args,
            ),
            ClientCommand::Window(window_args) => handlers::window(
                &cmd,
                &cmd.make_print_or_file_handler(&window_args.output)?,
                window_args,
            ),
            ClientCommand::Repl(repl_args) => handlers::repl(Arc::clone(&cmd), repl_args),
            ClientCommand::Bench(bench_args) => handlers::bench(Arc::clone(&cmd), bench_args),
        };

        // As this is a oneshot and shouldn't be called anywhere else, we don't care about the result
        tracing::info!("Request done, sending done notification");
        _ = cmd.done_send.try_send(());

        response
    }

    fn make_print_handler(&self) -> Res {
        Arc::new(Mutex::new(ResponseHandler::Print))
    }

    fn make_file_handler(&self, path: impl AsRef<Path>) -> Result<Res> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Arc::new(Mutex::new(ResponseHandler::File(file))))
    }

    /// Helper function to get an appropriate print or file handler, depending on whether a file path was provided.
    fn make_print_or_file_handler(&self, output: &Option<impl AsRef<Path>>) -> Result<Res> {
        if let Some(output_file) = &output {
            self.make_file_handler(output_file)
        } else {
            Ok(self.make_print_handler())
        }
    }

    /// Send a request for dispatch.
    ///
    /// Will block until the request channel has capacity.
    #[tracing::instrument(skip_all)]
    pub fn send(&self, req: Request, res: &Res) -> Result<()> {
        tracing::debug!("Sent request: {:?}", req);

        // Get an id and bump to the next one
        // We are just using this as an id so we can safely use Relaxed ordering here
        let id = self.next_req_id.fetch_add(1, Ordering::Relaxed);

        // Track the request before sending so we know when we have received responses
        self.tracker.track(id, &req, Arc::clone(&res));
        self.request_tx.send((id, req))?;

        Ok(())
    }

    /// Report the number of outstanding requests in the tracker.
    pub fn outstanding(&self) -> usize {
        self.tracker.outstanding()
    }
}

/// These are the unique ways that incoming responses will be handles, organized by the type of handling.
#[derive(Default)]
pub enum ResponseHandler {
    #[default]
    Print,
    File(File),
    ReplPrint(Sender<()>),
    ReplFile(Sender<()>, File),
    Bench(Option<Duration>),
}

impl ResponseHandler {
    #[tracing::instrument(skip_all)]
    pub fn handle(&mut self, id: u32, done: Option<Duration>, res: &Response) -> Result<()> {
        tracing::debug!("Handling response: {:?}", res);

        match self {
            Self::Print => {
                let stdout = std::io::stdout();
                write_response(id, done, res, &mut stdout.lock())?;
            }
            Self::File(file) => {
                write_response(id, done, res, file)?;
            }
            Self::ReplPrint(sender) => {
                let stdout = std::io::stdout();
                write_response(id, done, res, &mut stdout.lock())?;
                if done.is_some() {
                    sender.send(()).expect("channel disconnected");
                }
            }
            Self::ReplFile(sender, file) => {
                write_response(id, done, res, file)?;
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

        Ok(())
    }
}

/// Write to the passed writer with a short logging preamble.
macro_rules! writeln_with_preamble {
    ($writer:expr, $req_id:expr, $duration:expr, $($arg:tt)*) => {
        (|| -> std::io::Result<()> {
            write!($writer, "[req {}", $req_id)?;
            if let Some(duration) = $duration {
                write!($writer, "; {}ms", duration.as_millis())?;
            }
            write!($writer, "] ")?;
            writeln!($writer, $($arg)*)
        })()
    };
}

fn write_response<W: Write>(
    id: u32,
    duration: Option<Duration>,
    res: &Response,
    data_writer: &mut W,
) -> Result<(), std::io::Error> {
    let mut stderr = std::io::stderr().lock();

    match res {
        Response::Success(Some(msg)) => writeln_with_preamble!(stderr, id, duration, "{}", msg),
        Response::Success(None) => writeln_with_preamble!(stderr, id, duration, "success"),
        Response::Done(n) => {
            writeln_with_preamble!(stderr, id, duration, "done with {} responses", n)
        }
        Response::Stats(n) => writeln_with_preamble!(
            stderr,
            id,
            duration,
            "QT size={}; key: {:?}; bytes sent={}; bytes recv={}",
            n.qt_size,
            n.key_mode,
            n.bytes_sent,
            n.bytes_recv
        ),
        Response::ResultCounts { success, fail } => {
            writeln_with_preamble!(stderr, id, duration, "succeed {}, failed {}", success, fail)
        }
        Response::BasicResults(results) => {
            for result in results {
                match result {
                    Ok(basic_result) => {
                        let content_json = format_content(&basic_result.content);
                        writeln!(data_writer, "{},{}", basic_result.id, content_json)?;
                    }
                    Err(err) => {
                        writeln!(stderr, "Error: {}", err)?;
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
                        let _ = writeln!(
                            data_writer,
                            "{},{},{},{}",
                            proximity_result.input_index,
                            proximity_result.id,
                            proximity_result.distance,
                            content_json
                        )?;
                    }
                    Err(err) => {
                        writeln_with_preamble!(stderr, id, duration, "Error: {}", err)?;
                    }
                }
            }
            Ok(())
        }
        Response::Error(msg) => writeln_with_preamble!(stderr, id, duration, "error: {}", msg),
        Response::Bench(_) => unreachable!(),
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
