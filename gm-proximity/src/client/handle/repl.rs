use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{bail, Error, Result};
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select};

use crate::{args::ResetArgs, message::prelude::*};

use super::{handlers, CommandHandler, ResponseHandler};

/// REPL command handler.
///
/// Start an interactive prompt for the client.
pub fn repl(handler: &mut CommandHandler) -> Result<()> {
    // Set the special repl response handler
    let (send, recv) = crossbeam::channel::bounded(0);
    handler.reponse_handler = ResponseHandler::Repl(send);

    // Set standard get/delete settings
    let mut settings = Settings::default();

    // Our REPL loop has to push dispatch onto a separate task/thread as we otherwise cannot recieve while sending
    // In general this won't matter, but it is theoretically possible that the buffers all fill with outgoing messages
    // before the sending is done, which will effectively deadlock the repl if we can't process receipts
    // We only need to do this when the potential data volume is large
    // TODO: Is there a more efficient way to do this without spinning up a new thread?
    loop {
        // First dispatch all the requests
        match select_command()? {
            // Stats
            Some(0) => {
                handler.send(Request::Stats)?;
            }
            // Reset
            Some(1) => {
                if let Some(reset_args) = build_reset() {
                    handlers::reset(handler, reset_args)?;
                }
            }
            // Load
            // Spawn this in a new thread
            Some(2) => {
                // We still wrap this in if-let because the handler treats None as stdin but we don't want to do that in
                // this case
                // TODO: Spawn this on a thread
                if let Some(path) = build_path() {
                    handlers::load(handler, Some(path))?;
                }
            }
            // Get
            Some(3) => {
                // For Get, we only send a single request
                if let Some(get_req) = build_get(&mut settings) {
                    handler.send(Request::Get(get_req))?;
                }
            }
            // Delete
            Some(4) => {
                // For Delete, we only send a single request
                if let Some(key_set) = build_delete(&mut settings) {
                    handler.send(Request::Delete(key_set))?;
                }
            }
            // Knn
            Some(5) => {}
            // Window
            Some(6) => {}
            // Change settings
            Some(7) => change_settings(&mut settings),
            None => {
                if confirm_quit()? {
                    // TODO: This seems to be causing a panic due to a closed channel if any work has been done
                    eprintln!("Quitting");
                    break;
                }
            }
            _ => unreachable!(),
        }

        // Then recieve responses - we need to check timeout and outstanding count to avoid locking up
        // TODO: Could interupt this loop with a confirm quit if a long time has elapsed between messages
        while handler.outstanding() > 0 {
            recv.recv_timeout(Duration::from_millis(100))
                .expect("channel disconnected");
        }
    }

    Ok(())
}

#[derive(Default)]
struct Settings {
    query_key_type: QueryKeyType,
    meta_only: bool,
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
            QueryKeyType::Uid => "Uid",
            QueryKeyType::Custom => "Custom",
        }
    }

    fn list() -> [&'static str; 2] {
        [QueryKeyType::Uid.as_str(), QueryKeyType::Custom.as_str()]
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

// TODO: Enable fuzzy select here?
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

fn change_settings(settings: &mut Settings) {
    eprintln!("Changing settings used for key types and retrieval options");

    let key_type: QueryKeyType = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Retrieval key type")
        .items(&QueryKeyType::list())
        .default(settings.query_key_type as usize)
        .interact()
        .map_err(Error::new)
        .and_then(TryInto::try_into)
        .unwrap();

    settings.query_key_type = key_type;
    eprintln!("Updated query key type: {:?}", key_type);

    let meta_only = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Retrieve meta only")
        .default(settings.meta_only)
        .wait_for_newline(true)
        .interact()
        .unwrap();

    settings.meta_only = meta_only;
}

// TODO: Consider a broader range of return values from here
fn build_path() -> Option<PathBuf> {
    eprintln!("If directory attempts to FZF, if file loads it straight; . for cwd");
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

fn build_reset() -> Option<ResetArgs> {
    let key_type = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("What key type?")
        .items(&["Auto", "Custom Increment", "Custom Value"])
        .default(0)
        .interact()
        .unwrap();

    let json_ptr: Option<String> = if key_type > 0 {
        let ptr = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("JSON pointer for custom key")
            .interact_text()
            .unwrap();
        Some(ptr)
    } else {
        None
    };

    let (key_int, key_bytes) = match key_type {
        0 => (None, None),
        1 => (json_ptr, None),
        2 => (None, json_ptr),
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
            force: true,
        })
    } else {
        eprintln!("Reset aborted");
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
                0 => change_settings(settings),
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

fn build_get(settings: &mut Settings) -> Option<GetReq> {
    eprintln!(
        "Enter key ({}) to get, blank to change settings or abort",
        settings.id_str()
    );

    if let Some(keys) = key_set_loop(settings) {
        Some(GetReq {
            keys,
            meta_only: settings.meta_only,
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

use std::process::{Command, Stdio};

// TODO: Improve this handling, perhaps add a setting for a command
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
