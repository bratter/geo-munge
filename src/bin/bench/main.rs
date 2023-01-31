mod args;
mod commands;
mod run;
mod write;

use std::{
    env::{self, VarError},
    fs,
    path::PathBuf,
};

use args::{Args, Command};
use commands::{build, clean, execute, list, load, remove};
use geo_munge::error::Error;

use clap::Parser;

// TODO: See notes in execute command file on further improvements
#[derive(Debug)]
pub struct Paths {
    pub run: PathBuf,
    pub build: PathBuf,
}

const SUBDIR: &str = "proximity_bench";

// The bench binary in only compiled on linux
#[cfg(target_os = "linux")]
fn main() -> Result<(), Error> {
    // On startup, get the directories for the run info and the build data
    // These are $XDG_DATA_HOME and $XDG_STATE_HOME respectively
    let paths = make_paths()?;

    match Args::parse().command {
        Command::List => list(&paths.run),
        Command::Load { path } => load(&path, &paths.run),
        Command::Remove { name } => remove(&name, &paths),
        Command::Clean => clean(&paths.build),
        Command::Build { name } => build(&name, &paths),
        Command::Execute { name, n, bin } => execute(&name, n, bin, &paths),
    }
}

// Basic error message for non-linux
#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("The bench binary is only built for linux");
}

fn make_paths() -> Result<Paths, Error> {
    // Get the home dir as a PathBuf, and make the alt paths
    let home_env = env::var("HOME").map_err(|_| Error::CannotReadFile(PathBuf::from("$HOME")))?;

    let mut run_alt = PathBuf::from(&home_env);
    run_alt.push(".local/share");
    let mut build_alt = PathBuf::from(&home_env);
    build_alt.push(".local/state");

    Ok(Paths {
        run: setup_dir("XDG_DATA_HOME", run_alt)?,
        build: setup_dir("XDG_STATE_HOME", build_alt)?,
    })
}

fn setup_dir(env: &str, alt: PathBuf) -> Result<PathBuf, Error> {
    let path = env::var(env);
    let mut path = match path {
        Ok(ref path) if path.len() > 0 => PathBuf::from(path),
        Ok(_) => alt,
        Err(VarError::NotPresent) => alt,
        Err(VarError::NotUnicode(_)) => return Err(Error::CannotReadFile(PathBuf::from(env))),
    };

    // Add our application's specific directory
    path.push(SUBDIR);

    fs::create_dir_all(&path).map_err(|_| Error::CannotReadFile(path.clone()))?;

    Ok(path)
}
