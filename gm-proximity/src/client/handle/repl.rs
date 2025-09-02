use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use anyhow::{bail, Error, Result};
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select};

use crate::{
    args::{KnnArgs, ReplArgs, ResetArgs},
    message::prelude::*,
};

use super::{handlers, CommandHandler, OutputFormat, OutputOptions, ResponseHandler};

/// REPL command handler.
///
/// Start an interactive prompt for the client.
#[tracing::instrument(skip_all)]
pub fn repl(handler: Arc<CommandHandler>, repl_args: ReplArgs) -> Result<()> {
    // Create the special repl response handler
    let (send, recv) = crossbeam::channel::bounded(0);
    let res = if let Some(output_file) = &repl_args.output {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(output_file)?;
        ResponseHandler::ReplFile(send, file)
    } else {
        ResponseHandler::ReplPrint(send)
    };
    let res = Arc::new(Mutex::new(res));

    // Set standard get/delete settings
    let mut settings = Settings::default();

    // Set up the running state flag for use with spawned tasks
    // This will be set to false here, then in spawned threads, we set true, run the task, then reset to false
    let waiting = Arc::new(AtomicBool::new(false));

    // Our REPL loop has to push dispatch onto a separate task/thread as we otherwise cannot recieve while sending
    // In general this won't matter, but it is theoretically possible that the buffers all fill with outgoing messages
    // before the sending is done, which will effectively deadlock the repl if we can't process receipts
    // We only need to do this when the potential data volume is large
    loop {
        tracing::debug!("Next REPL loop");

        // Beacuse of the spawn requirements we need a join handle
        let mut join_handle = None;

        // First dispatch all the requests
        let command_result = match select_command()? {
            // Stats
            Some(0) => handler.send(Request::Stats, &res),
            // Reset
            Some(1) => {
                if let Some(reset_args) = build_reset() {
                    handlers::reset(&handler, &res, reset_args)
                } else {
                    Ok(())
                }
            }
            // Load
            // Spawn this in a new thread
            Some(2) => {
                // We still wrap this in if-let because the handler treats None as stdin but we don't want to do that in
                // this case
                if let Some(path) = build_path() {
                    let th = Arc::clone(&handler);
                    let w = Arc::clone(&waiting);
                    let r = Arc::clone(&res);
                    waiting.store(true, Ordering::Release);
                    join_handle = Some(std::thread::spawn(move || {
                        let handle_result = handlers::load(&th, &r, Some(path));
                        w.store(false, Ordering::Release);

                        handle_result
                    }));
                }
                Ok(())
            }
            // Get
            Some(3) => {
                // For Get, we only send a single request
                if let Some(get_req) = build_get(&mut settings) {
                    handler.send_with_output(Request::Get(get_req), &res, settings.output_options)
                } else {
                    Ok(())
                }
            }
            // Delete
            Some(4) => {
                // For Delete, we only send a single request
                if let Some(key_set) = build_delete(&mut settings) {
                    handler.send(Request::Delete(key_set), &res)
                } else {
                    Ok(())
                }
            }
            // Knn
            Some(5) => {
                // For Knn, we want to use the handler's request sending logic and also spawn on a thread as the data
                // flow could be large if we use file input
                if let Some(knn_args) = build_knn(&mut settings) {
                    let th = Arc::clone(&handler);
                    let w = Arc::clone(&waiting);
                    let r = Arc::clone(&res);
                    waiting.store(true, Ordering::Release);
                    join_handle = Some(std::thread::spawn(move || {
                        let handle_result = handlers::knn(&th, &r, knn_args);
                        w.store(false, Ordering::Release);

                        handle_result
                    }));
                }
                Ok(())
            }
            // Window
            Some(6) => {
                // For window we can just send a single request
                if let Some(window_req) = build_window(&settings) {
                    handler.send_with_output(
                        Request::Window(window_req),
                        &res,
                        settings.output_options,
                    )
                } else {
                    Ok(())
                }
            }
            // Change settings
            Some(7) => change_settings(&mut settings),
            None => {
                if confirm_quit()? {
                    // TODO: This seems to be causing a panic due to a closed channel if any work has been done
                    eprintln!("Quitting");
                    break;
                } else {
                    Ok(())
                }
            }
            _ => unreachable!(),
        };

        if let Err(err) = command_result {
            // TODO: Would like to make this a palette color, could add ;38;5;x where x is the color index, but need to
            // find the right one; or ;31 is red, but is it the default palette red?
            eprintln!("\x1b[1mCommand error:\x1b[0m {}", err);
        }

        // Then receive responses - we need to check timeout and outstanding count to avoid locking up
        // TODO: Could interupt this loop with a confirm quit if a long time has elapsed between messages
        tracing::debug!(
            "Entering response loop with {} outstanding with wait status {}",
            handler.outstanding(),
            waiting.load(Ordering::Acquire),
        );
        while handler.outstanding() > 0 || waiting.load(Ordering::Acquire) {
            recv.recv_timeout(Duration::from_millis(100))
                .expect("channel disconnected");
        }
        tracing::debug!("Received all responses");

        // Close out the join handle if we reach the bottom of the loop. This shouldn't block as the waiting flag will
        // be the last thing run before the closure returns
        if let Some(handle) = join_handle {
            if let Err(err) = handle.join().expect("Couldn't join thread") {
                eprintln!("\x1b[1mCommand error:\x1b[0m {}", err);
            }
            tracing::debug!("Joined handle from spawned task thread");
        }
    }

    Ok(())
}

#[derive(Default)]
struct Settings {
    query_data_type: QueryDataType,
    query_key_type: QueryKeyType,
    output_options: OutputOptions,
}

impl Settings {
    fn id_str(&self) -> &str {
        if self.query_key_type == QueryKeyType::Uid {
            "Uid"
        } else {
            "Custom"
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum QueryDataType {
    #[default]
    Key = 0,
    Geometry = 1,
}

impl QueryDataType {
    fn as_str(&self) -> &str {
        match self {
            Self::Key => "Key",
            Self::Geometry => "Geometry",
        }
    }

    fn list() -> [&'static str; 2] {
        [Self::Key.as_str(), Self::Geometry.as_str()]
    }
}

impl TryFrom<usize> for QueryDataType {
    type Error = Error;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Key),
            1 => Ok(Self::Geometry),
            _ => bail!("Invalid index for QueryDataValue"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum QueryKeyType {
    #[default]
    Uid = 0,
    Custom = 1,
}

impl QueryKeyType {
    fn is_bytes(&self) -> bool {
        if self == &Self::Custom {
            true
        } else {
            false
        }
    }

    fn as_str(&self) -> &str {
        match self {
            Self::Uid => "Uid",
            Self::Custom => "Custom",
        }
    }

    fn list() -> [&'static str; 2] {
        [Self::Uid.as_str(), Self::Custom.as_str()]
    }
}

impl TryFrom<usize> for QueryKeyType {
    type Error = Error;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Uid),
            1 => Ok(Self::Custom),
            _ => bail!("Invalid index for QueryKeyValue"),
        }
    }
}

// TODO: Enable fuzzy select feature for use here?
fn select_command() -> Result<Option<usize>> {
    let items = [
        "Stats", "Reset", "Load", "Get", "Delete", "Knn", "Window", "Settings",
    ];
    let selected = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Choose a command")
        .items(&items)
        .interact_opt()?;

    Ok(selected)
}

fn confirm_quit() -> Result<bool> {
    let conf = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Do you want to quit?")
        .interact()?;

    Ok(conf)
}

fn change_settings(settings: &mut Settings) -> Result<()> {
    eprintln!("Select a setting to change, note that even when cancelled with <esc>, previous selections are changed");

    loop {
        let setting = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Select a setting to change (esc/q for main menu)")
            .items(&[
                "Spatial operation (use keys or geoms for neighbor queries)",
                "Key type (use Uids or Custom keys for id-based queries)",
                "Content mode (the geojson data to return from queries)",
                "Output format (whether to return data as all json or partial csv/json)",
                "Done changing settings (also, esc/q)",
            ])
            .interact_opt()
            .unwrap();

        match setting {
            Some(0) => {
                let data_type = Select::with_theme(&ColorfulTheme::default())
                    .with_prompt("Spatial operation type")
                    .items(&QueryDataType::list())
                    .default(settings.query_data_type as usize)
                    .interact()
                    .map_err(Error::new)
                    .and_then(TryInto::try_into)
                    .unwrap();

                settings.query_data_type = data_type;
                eprintln!("Updated input data type: {}", data_type.as_str());
            }
            Some(1) => {
                // TODO: Should we not allow this setting when there is no custom key on the server
                let key_type: QueryKeyType = Select::with_theme(&ColorfulTheme::default())
                    .with_prompt("Retrieval key type")
                    .items(&QueryKeyType::list())
                    .default(settings.query_key_type as usize)
                    .interact()
                    .map_err(Error::new)
                    .and_then(TryInto::try_into)
                    .unwrap();

                settings.query_key_type = key_type;
                eprintln!("Updated query key type: {}", key_type.as_str());
            }
            Some(2) => {
                let content_mode: ContentMode = Select::with_theme(&ColorfulTheme::default())
                    .with_prompt("Content response mode for KNN and Window queries")
                    .items(&ContentMode::list())
                    .default(settings.output_options.content_mode as usize)
                    .interact()
                    .map_err(Error::new)
                    .and_then(TryInto::try_into)
                    .unwrap();

                settings.output_options.content_mode = content_mode;
            }
            Some(3) => {
                let mut opts = settings.output_options;

                opts.output_format = Select::with_theme(&ColorfulTheme::default())
                    .with_prompt("Output format mode for data-returning queries")
                    .items(&OutputFormat::list())
                    .default(opts.output_format as usize)
                    .interact()
                    .map_err(Error::new)
                    .and_then(TryInto::try_into)
                    .unwrap();

                if opts.output_format == OutputFormat::Csv {
                    opts.header = Confirm::with_theme(&ColorfulTheme::default())
                        .with_prompt("Show headers")
                        .default(opts.header)
                        .interact()
                        .unwrap();

                    opts.escape = Confirm::with_theme(&ColorfulTheme::default())
                        .with_prompt("Escape JSON in CSV")
                        .default(opts.escape)
                        .interact()
                        .unwrap();
                }

                settings.output_options = opts;
            }
            Some(4) | None => break,
            _ => unreachable!(),
        };
    }

    Ok(())
}

fn build_reset() -> Option<ResetArgs> {
    let key_type = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("What key type (esc/q to cancel)?")
        .items(&["Auto", "Custom Increment", "Custom Value", "GeoJSON Id"])
        .default(0)
        .interact_opt()
        .unwrap()?;

    let json_ptr: Option<String> = if key_type == 1 || key_type == 2 {
        let ptr = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("JSON pointer for custom key")
            .interact_text()
            .unwrap();
        Some(ptr)
    } else {
        None
    };

    let (key_int, key_bytes, key_json_id) = match key_type {
        0 => (None, None, false),
        1 => (json_ptr, None, false),
        2 => (None, json_ptr, false),
        3 => (None, None, true),
        _ => unreachable!(),
    };

    let bbox = loop {
        let bbox_raw: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Enter bounding box (blank for default)")
            .allow_empty(true)
            .interact_text()
            .unwrap();

        if bbox_raw.len() == 0 {
            break None;
        } else {
            match bbox_raw.parse() {
                Ok(bbox) => break Some(bbox),
                Err(err) => eprintln!("{}", err),
            }
        }
    };

    if Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Confirm geo store reset")
        .interact()
        .ok()?
    {
        eprintln!("Resetting geo store");
        Some(ResetArgs {
            bbox,
            key_int,
            key_bytes,
            key_json_id,
            force: true,
        })
    } else {
        eprintln!("Reset aborted");
        None
    }
}

// TODO: Because the purpose of get is to retrieve the item, should we decouple this from the setting?
fn build_get(settings: &mut Settings) -> Option<GetReq> {
    eprintln!(
        "Enter key ({}) to get, blank to change settings or abort",
        settings.id_str()
    );

    if let Some(keys) = key_set_loop(settings) {
        Some(GetReq {
            keys,
            content_mode: settings.output_options.content_mode,
        })
    } else {
        None
    }
}

fn build_delete(settings: &mut Settings) -> Option<KeySet> {
    eprintln!(
        "Enter key ({}) to delete, blank to change settings or abort",
        settings.id_str()
    );

    key_set_loop(settings)
}

/// Build a knn argument set outside the context of clap.
///
/// Care must be taken to replicate clap exclusion rules, etc.
///
/// TODO:Consider pushing parsing the KnnArgs to the Args module and create a parsed variant here for use in the handler
fn build_knn(settings: &mut Settings) -> Option<KnnArgs> {
    // Create the args struct to build over the course of the builder.
    let mut args = KnnArgs::new(settings.output_options);

    args.k = loop {
        let k_raw: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Enter k nearest neighbors to retrieve")
            .interact_text()
            .unwrap();

        match k_raw.parse() {
            Ok(k) => break k,
            Err(err) => eprintln!("{}", err),
        }
    };

    args.r = loop {
        let r_raw: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Enter max search radius (blank for unbounded)")
            .allow_empty(true)
            .interact_text()
            .unwrap();

        if r_raw.len() == 0 {
            break None;
        } else {
            match r_raw.parse() {
                Ok(r) => break Some(r),
                Err(err) => eprintln!("{}", err),
            }
        }
    };

    const ABORT_MSG: &str = "(abort to change settings)";
    let (key_uid, key_bytes) = match (settings.query_data_type, settings.query_key_type) {
        (QueryDataType::Geometry, _) => {
            eprintln!("Querying with geometry {}", ABORT_MSG);
            (false, false)
        }
        (QueryDataType::Key, QueryKeyType::Uid) => {
            eprintln!(
                "Querying with {} keys {}",
                settings.query_key_type.as_str(),
                ABORT_MSG
            );
            (true, false)
        }
        (QueryDataType::Key, QueryKeyType::Custom) => {
            eprintln!(
                "Querying with {} keys {}",
                settings.query_key_type.as_str(),
                ABORT_MSG
            );
            (false, true)
        }
    };
    args.key_uid = key_uid;
    args.key_bytes = key_bytes;

    // TODO: Would like to directly use KeySet here, but doesn't work as it currently stands with the handler setup
    eprintln!("Enter data in the appropriate format or blank to abort or choose a file");
    let raw_data: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Data")
        .allow_empty(true)
        .interact_text()
        .unwrap();

    let (data, input) = if raw_data.len() == 0 {
        match Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Abort or choose file")
            .items(&["Abort", "Choose File"])
            .default(0)
            .interact()
            .unwrap()
        {
            0 => return None,
            1 => {
                // We return if the path is None rather than injecting null into the file as this represents an error
                // case where we have no file or data
                let path = build_path();
                if path.is_none() {
                    return None;
                }
                (None, path)
            }
            _ => unreachable!(),
        }
    } else {
        (Some(raw_data), None)
    };
    args.data = data;
    args.input = input;

    Some(args)
}

fn build_window(settings: &Settings) -> Option<WindowReq> {
    let join = match Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Choose selection mode")
        .items(&["Contains", "Intersects"])
        .default(0)
        .interact()
        .unwrap()
    {
        0 => JoinType::Contains,
        1 => JoinType::Intersects,
        _ => unreachable!(),
    };

    let bbox = loop {
        let bbox_raw: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Enter bounding box (blank to abort window query)")
            .allow_empty(true)
            .interact_text()
            .unwrap();

        if bbox_raw.len() == 0 {
            return None;
        } else {
            match bbox_raw.parse::<DegreeBbox>() {
                Ok(bbox) => break bbox,
                Err(err) => eprintln!("{}", err),
            }
        }
    };

    Some(WindowReq {
        bbox,
        join,
        content_mode: settings.output_options.content_mode,
    })
}

// TODO: Consider a broader range of return values from here
fn build_path() -> Option<PathBuf> {
    eprintln!("If directory attempts to FZF, if file loads it straight; . for cwd");
    if let Ok(dir) = std::env::current_dir() {
        eprintln!("Current working directory is {}", dir.to_string_lossy());
    }

    let path: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Path to load")
        .interact_text()
        .unwrap();
    let path = Path::new(&path);

    match path.try_exists() {
        // continue
        Ok(true) => {}
        Ok(false) => {
            eprintln!("Path doesn't exist, aborting");
            return None;
        }
        Err(err) => {
            eprintln!("{}", err);
            return None;
        }
    }

    if path.is_dir() {
        let canonical = match fs::canonicalize(path) {
            Ok(path) => path,
            Err(err) => {
                eprintln!("{}", err);
                return None;
            }
        };

        match run_fzf_in_dir(&canonical) {
            Ok(Some(path)) => {
                eprintln!("Load {}", path.to_string_lossy());
                match Confirm::with_theme(&ColorfulTheme::default())
                    .with_prompt("Confirm")
                    .default(true)
                    .interact()
                    .unwrap()
                {
                    true => Some(path),
                    false => None,
                }
            }
            Ok(None) => None,
            Err(err) => {
                eprintln!("FZF/find not available or other io error: {}", err);
                eprintln!(
                    "Check if fzf and find are on the system, if not you must enter a file only"
                );
                None
            }
        }
    } else if path.is_file() {
        eprintln!("Load {}", path.to_string_lossy());
        match Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Confirm")
            .default(true)
            .interact()
            .unwrap()
        {
            true => Some(PathBuf::from(path)),
            false => None,
        }
    } else {
        eprintln!("Provided path was neither a directory or a file");
        None
    }
}

/// Utility to build a [`KeySet`] while also enable setting changes.
fn key_set_loop(settings: &mut Settings) -> Option<KeySet> {
    loop {
        let key: String = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Choose keys")
            .allow_empty(true)
            .interact_text()
            .unwrap();

        if key.len() == 0 {
            match Select::with_theme(&ColorfulTheme::default())
                .with_prompt("Select action")
                .items(&["Change Settings", "Return", "Abort"])
                .default(0)
                .interact()
                .unwrap()
            {
                0 => {
                    _ = change_settings(settings);
                }
                1 => {}
                2 => break None,
                _ => unreachable!(),
            }
        } else {
            match KeySet::parse_with_type(&key, settings.query_key_type.is_bytes()) {
                Ok(ks) => break Some(ks),
                Err(err) => eprintln!("{}", err),
            }
        }
    }
}

// TODO: Improve this handling, perhaps add a setting for a command, also need to make work in windows
fn run_fzf_in_dir<P: AsRef<Path>>(dir: &P) -> std::io::Result<Option<PathBuf>> {
    let find = Command::new("find")
        .arg(".")
        .current_dir(dir)
        .stdout(Stdio::piped())
        .spawn()?;

    let fzf = Command::new("fzf")
        .stdin(find.stdout.expect("Stdout not available"))
        .stdout(Stdio::piped())
        .spawn();

    let fzf = match fzf {
        Ok(child) => child,
        Err(_) => return Ok(None), // fzf not installed
    };

    let output = fzf.wait_with_output()?;
    if output.status.success() {
        let selected = String::from_utf8_lossy(&output.stdout);
        Ok(Some(dir.as_ref().join(selected.trim())))
    } else {
        Ok(None)
    }
}
