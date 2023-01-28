mod args;
mod commands;
mod run;

use std::{
    env::{self, VarError},
    fs,
    path::PathBuf,
};

use args::{Args, Command};
use commands::{build, clean, list, load, remove};
use geo_munge::error::Error;

use clap::Parser;

// TODO: Make a benchmarking rig:
//       - Separate dataset generation and qt execution so it can be used by perf etc.
//       - Configurable in some sort of run format, done in a config file, needs to be able to
//       manage combinations of geo data and cmp point data, although cmp point can probably be
//       simple-ish
//       - Should test some level of metadata stuff as this may take time per run
//       - Data generation only needs to build a single shape type per file, so can encode in
//       shapefiles as a starting point, but might also be interesting to see how long parsing kml
//       or geojson takes
//       - Cmp points can just be a simple lng-lat in
//       - Use $XDG_STATE_HOME (there is a default if not set: $HOME/.local/state
//       when done
//       - Output timing results to a csv file
//       - A single run needs to be the combination of a data (shp) and cmp (csv) files
//       - Required settings for data:
//          - shape (start with point, probably only add linestrings, others aren't necessary)
//          - count
//          - meta (don't over invest - just a count of the number of text fields, all short)
//          - coords (some guide to the number of coords to use in linestrings, polygons)
//          - complexity (not for now - how to distribute coords in linestrings, etc)
//          - distribution (not for now - how to distribute shapes in the bbox)
//      - Required settings for cmp:
//          - count
//      - Set up the files in folders with a run id, and just keep track of the inputs for the run
//      - Run as a whole needs repeats, but should be quite stable
//      in a tracking file have an option to delete a run, otherwise can't overwrite, so load run
//      definition from a json file, which checks and stores it, then generate, then execute.
//      - The executor will need to just run Command to execute proximity
//      - This will be fine for my experimental purposes, but to run with perf may need to justdo
//      the generation here then run perf straight on proximity
//
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
        Command::Execute { name } => todo!(),
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
