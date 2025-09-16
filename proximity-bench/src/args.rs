use clap::Parser;

/// Benchmarking tool for testing proximity-based geospatial operations.
#[derive(Debug, Parser)]
pub struct Args {
    /// Total amount of data to test with **in Mb**. Doesn't include overhead.
    pub total_data: Option<usize>,

    #[clap(long, short = 'q')]
    pub request_size: Option<u32>,

    #[clap(long, short = 'r')]
    pub response_size: Option<u32>,

    #[clap(long, short = 't')]
    pub response_ratio: Option<u32>,

    /// Delay to simulate processing time to generate **each response** on the server. This applies to each response,
    /// not request, so when the response ratio is >1 this delay will apply multiple times to a single request.
    #[clap(long, short = 'p')]
    pub handle_delay: Option<u64>,

    #[clap(long, short = 's')]
    pub send_delay: Option<u64>,

    #[clap(long, short = 'v')]
    pub receive_delay: Option<u64>,
}

