use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Command line utility to run test cases for the proximity binary.
#[derive(Parser, Debug)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// List the currently available run configs.
    List,

    /// Load a set of run data from the passed json file.
    Load {
        /// Path to the json file to check and load.
        path: PathBuf,
    },

    /// Remove a run config.
    Remove {
        /// The name of the config to remove
        name: String,
    },

    /// Remove any runtime build data.
    Clean,

    /// Build the assets for a specific run, but don't execute.
    Build {
        /// The loaded run to build for.
        name: String,
    },

    /// Build assets for, then execute a specific run. This command will generate and remove the
    /// assets for each stage in the run as it goes, so it does not cause too much buildup.
    Execute {
        /// The run to execute.
        name: String,

        /// Number of times to execute each run
        #[arg(short, default_value = "1")]
        n: usize,

        /// Location of the proximity binary. If not provided first take the binary in the pwd, it
        /// it exists, otherwise falls back to one on PATH.
        #[arg(long)]
        bin: Option<PathBuf>,
    },
}
