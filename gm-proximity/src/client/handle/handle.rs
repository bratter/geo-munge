use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
    str::FromStr,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use anyhow::{bail, Error, Result};
use crossbeam::channel::{self, Receiver, Sender};
use protocol::prelude::*;

use crate::args::ClientCommand;

use super::{handlers, Res, Tracker};

/// DTO for Response metadata
pub struct ResponseMeta {
    pub req_id: u32,
    pub res_id: u32,
    pub done: Option<Duration>,
    pub output_options: OutputOptions,
}

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
        tracing::debug!("Request done, sending done notification");
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
        self.send_with_output(req, res, OutputOptions::default())
    }

    /// Send a request for dispatch with additional output format information.
    ///
    /// Will block until the request channel has capacity. Will add contextual information to the tracker so responses
    /// follow the right format.
    #[tracing::instrument(skip_all)]
    pub fn send_with_output(&self, req: Request, res: &Res, out_opts: OutputOptions) -> Result<()> {
        tracing::debug!("Sent request: {:?}", req);

        // Get an id and bump to the next one
        // We are just using this as an id so we can safely use Relaxed ordering here
        let req_id = self.next_req_id.fetch_add(1, Ordering::Relaxed);

        // Track the request before sending so we know when we have received responses
        self.tracker.track(req_id, &req, Arc::clone(&res), out_opts);
        self.request_tx.send((req_id, req))?;

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
    pub fn handle(&mut self, meta: ResponseMeta, mut res: Response) -> Result<()> {
        tracing::debug!("Handling response: {:?}", res);

        match self {
            Self::Print => {
                let stdout = std::io::stdout();
                write_response(&meta, &mut res, &mut stdout.lock())?;
            }
            Self::File(file) => {
                write_response(&meta, &mut res, file)?;
            }
            Self::ReplPrint(sender) => {
                let stdout = std::io::stdout();
                write_response(&meta, &mut res, &mut stdout.lock())?;
                if meta.done.is_some() {
                    sender.send(()).expect("channel disconnected");
                }
            }
            Self::ReplFile(sender, file) => {
                write_response(&meta, &mut res, file)?;
                if meta.done.is_some() {
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
    ($writer:expr, $meta:expr, $($arg:tt)*) => {
        (|| -> std::io::Result<()> {
            write!($writer, "[req {}; res {}", $meta.req_id, $meta.res_id)?;
            if let Some(duration) = $meta.done {
                write!($writer, "; {}ms", duration.as_millis())?;
            }
            write!($writer, "] ")?;
            writeln!($writer, $($arg)*)
        })()
    };
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum OutputFormat {
    /// Return output as JSON.
    #[default]
    Json,

    /// Return output as csv with geometries or properties as JSON when the content_mode returns them.
    Csv,
}

impl OutputFormat {
    fn as_str(&self) -> &str {
        match self {
            Self::Json => "JSON",
            Self::Csv => "CSV",
        }
    }

    pub fn list() -> [&'static str; 2] {
        [Self::Json.as_str(), Self::Csv.as_str()]
    }
}

impl FromStr for OutputFormat {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "csv" => Ok(Self::Csv),
            _ => bail!("Invalid output format '{}'. Valid options are json, csv", s),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OutputOptions {
    /// Content mode for output data. for get requests will default to "full", but all others will default to returning
    /// the request metadata only.
    pub content_mode: ContentMode,

    pub output_format: OutputFormat,

    /// Render headers in csv output.
    pub header: bool,

    /// Escape JSON in CSV to ensure correct csv parsing.
    pub escape: bool,
}

impl Default for OutputOptions {
    fn default() -> Self {
        Self {
            content_mode: ContentMode::default(),
            output_format: OutputFormat::default(),
            header: true,
            escape: true,
        }
    }
}

// TODO: Like ContentMode, should these be moved to a common location?
impl TryFrom<usize> for OutputFormat {
    type Error = Error;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Json),
            1 => Ok(Self::Csv),
            _ => bail!("Invalid index for OutputFormat"),
        }
    }
}

fn write_response<W: Write>(
    meta: &ResponseMeta,
    res: &mut Response,
    data_writer: &mut W,
) -> Result<(), std::io::Error> {
    let mut stderr = std::io::stderr().lock();
    match res {
        Response::Success(Some(msg)) => {
            writeln_with_preamble!(stderr, meta, "{}", msg)
        }
        Response::Success(None) => {
            writeln_with_preamble!(stderr, meta, "success")
        }
        Response::Done(n) => {
            writeln_with_preamble!(stderr, meta, "done with {} responses", n)
        }
        Response::Stats(n) => {
            writeln_with_preamble!(
                stderr,
                meta,
                "GeoStore stats:\nUsing key: {:?} (bytes sent={}; bytes recv={})",
                n.key_mode,
                n.bytes_sent,
                n.bytes_recv
            )?;
            writeln!(stderr, "Count={}; Bbox={}", n.len, n.bbox)
        }
        Response::ResultCounts { success, fail } => {
            writeln_with_preamble!(stderr, meta, "succeed {}, failed {}", success, fail)
        }
        Response::BasicResults(results) => {
            for result in results {
                match (result, &meta.output_options.output_format) {
                    (Ok(basic_result), OutputFormat::Csv) => {
                        // When the output format is csv and we are in the first response for the request, we want to print a
                        // header row
                        if meta.res_id == 0 && meta.output_options.header {
                            write!(data_writer, "uid")?;
                            match &basic_result.content {
                                ContentType::FullFeature(_) | ContentType::GeometryOnly(_) => {
                                    writeln!(data_writer, ",feature")?;
                                }
                                ContentType::PropertiesOnly(_) => {
                                    writeln!(data_writer, ",properties")?;
                                }
                                ContentType::None => {
                                    writeln!(data_writer, "")?;
                                }
                            }
                        }

                        let content_json =
                            format_json(&basic_result.content, meta.output_options.escape);
                        write!(data_writer, "{}", basic_result.id)?;
                        if let Some(json) = content_json {
                            writeln!(data_writer, ",{}", json)?;
                        }
                    }
                    (Ok(basic_result), OutputFormat::Json) => {
                        let content = &mut basic_result.content;
                        content.set_property("_uid", basic_result.id);
                        writeln!(
                            data_writer,
                            "{}",
                            format_json(content, false).expect("ContentType is not None")
                        )?;
                    }
                    (Err(err), _) => {
                        writeln!(stderr, "Error: {}", err)?;
                    }
                }
            }
            Ok(())
        }
        Response::ProximityResults(results) => {
            for result in results {
                match (result, &meta.output_options.output_format) {
                    (Ok(proximity_result), OutputFormat::Csv) => {
                        // print header row as above
                        if meta.res_id == 0 && meta.output_options.header {
                            write!(
                                data_writer,
                                "input_index,input_uid,result_uid,distance_meters"
                            )?;
                            match &proximity_result.content {
                                ContentType::FullFeature(_) | ContentType::GeometryOnly(_) => {
                                    writeln!(data_writer, ",feature")?;
                                }
                                ContentType::PropertiesOnly(_) => {
                                    writeln!(data_writer, ",properties")?;
                                }
                                ContentType::None => {
                                    writeln!(data_writer, "")?;
                                }
                            }
                        }

                        let content_json =
                            format_json(&proximity_result.content, meta.output_options.escape);
                        write!(data_writer, "{},", proximity_result.input_index)?;
                        if let Some(input_uid) = proximity_result.input_uid {
                            write!(data_writer, "{},", input_uid)?;
                        } else {
                            write!(data_writer, ",")?;
                        }
                        write!(
                            data_writer,
                            "{},{}",
                            proximity_result.id, proximity_result.distance_meters,
                        )?;
                        if let Some(json) = content_json {
                            writeln!(data_writer, ",{}", json)?;
                        }
                    }
                    (Ok(proximity_result), OutputFormat::Json) => {
                        let content = &mut proximity_result.content;
                        content.set_property("_inputIndex", proximity_result.input_index);
                        if let Some(input_uid) = proximity_result.input_uid {
                            content.set_property("_inputUid", input_uid);
                        }
                        content.set_property("_resultUid", proximity_result.id);
                        content.set_property("_distanceMeters", proximity_result.distance_meters);

                        writeln!(
                            data_writer,
                            "{}",
                            format_json(content, false).expect("ContentType is not None")
                        )?;
                    }
                    (Err(err), _) => {
                        writeln_with_preamble!(stderr, meta, "Error: {}", err)?;
                    }
                }
            }
            Ok(())
        }
        Response::Error(msg) => writeln_with_preamble!(stderr, meta, "error: {}", msg),
        Response::Bench(_) => unreachable!(),
        // TODO: Change error print of unknown response types
        _ => writeln!(
            stderr,
            "Unknown response type for req={}, res={}",
            meta.req_id, meta.res_id
        ),
    }
}

// TODO: Write this rather than allocate
fn format_json(content: &ContentType, escape_json: bool) -> Option<String> {
    let json_str = match content {
        ContentType::FullFeature(feature) => Some(feature.to_string()),
        ContentType::GeometryOnly(feature) => Some(feature.to_string()),
        ContentType::PropertiesOnly(properties) => Some(properties.to_string()),
        ContentType::None => None,
    };

    // Ensure that the json strings are properly escaped in csv
    match (escape_json, &json_str) {
        (true, Some(json_str)) => Some(format!("\"{}\"", json_str.replace('"', "\"\""))),
        _ => json_str,
    }
}
