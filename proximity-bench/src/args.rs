use clap::Parser;

/// Benchmarking tool for testing proximity-based geospatial operations.
#[derive(Debug, Parser)]
pub struct Args {
    /// Path to the benchmark specification JSON file.
    /// If not found, will look in the /specs directory.
    pub spec_file: String,
}
