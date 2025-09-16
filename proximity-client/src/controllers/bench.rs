//! Benchmarking and load testing client handler.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, Result};
use protocol::prelude::*;

use crate::{args::BenchClient, handler::ResponseHandler};

use super::CommandHandler;

/// Benchmarking command handler
pub fn bench(handler: Arc<CommandHandler>, bench: BenchClient) -> Result<()> {
    // Set delays ands build the response handler
    let tx_delay = bench.send_delay.map(Duration::from_millis);
    let rx_delay = bench.receive_delay.map(Duration::from_millis);
    let res = Arc::new(Mutex::new(ResponseHandler::Bench(rx_delay)));

    // Determine the total data and the amount shipped per request
    // This is a duplicate of what we do in the wrapper
    let total_data = bench.total_data * 1024 * 1024;
    let request_count = total_data / (bench.request_size as usize);
    if request_count == 0 {
        bail!(
            "total data of {}mb too low for request size {}b",
            bench.total_data,
            bench.request_size
        );
    }

    for _ in 0..request_count {
        if let Some(t) = tx_delay {
            std::thread::sleep(t);
        }

        let mut data = Vec::with_capacity(bench.request_size as usize);
        data.resize(bench.request_size as usize, 0);
        let bench_req = BenchReq {
            delay: bench.handle_delay,
            size: bench.response_size,
            ratio: bench.response_ratio,
            data,
        };

        handler.send(Request::Bench(bench_req), &res)?;
    }

    Ok(())
}
