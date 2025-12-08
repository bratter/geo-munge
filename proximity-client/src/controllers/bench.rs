//! Benchmarking and load testing client handler.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, Result};
use protocol::prelude::*;

use crate::{args::BenchClient, handler::ResponseHandler};

use super::CommandHandler;

/// Benchmarking command handler
pub fn bench(handler: Arc<CommandHandler>, bench: BenchClient) -> Result<()> {
    // Set delays ands build the response handler
    let rx_delay = bench.receive_delay.map(Duration::from_millis);
    let res = Arc::new(Mutex::new(ResponseHandler::Bench(rx_delay)));

    // Determine the total data and the amount shipped per request
    // This is a duplicate of what we do in the wrapper
    // FIX: Push calc onto bench client? Take out of argument list
    let total_data = bench.total_data * 1024 * 1024;
    let request_count = total_data / (bench.request_size as usize);
    if request_count == 0 {
        bail!(
            "total data of {}mb too low for request size {}b",
            bench.total_data,
            bench.request_size
        );
    }

    // Handle file-based vs generated data
    if bench.data_file.is_some() {
        bench_with_file_data(handler, &bench, res, request_count)
    } else {
        bench_with_generated_data(handler, &bench, res, request_count)
    }
}

/// Benchmarking with generated empty data (original behavior)
fn bench_with_generated_data(
    handler: Arc<CommandHandler>,
    bench: &BenchClient,
    res: Arc<Mutex<ResponseHandler>>,
    request_count: usize,
) -> Result<()> {
    let tx_delay = bench.send_delay.map(Duration::from_millis);

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

/// Benchmarking with actual file data
/// FIX: Improve buffering logic, other general fixes
fn bench_with_file_data(
    handler: Arc<CommandHandler>,
    bench: &BenchClient,
    res: Arc<Mutex<ResponseHandler>>,
    request_count: usize,
) -> Result<()> {
    // Open and prepare file for reading
    let path = bench.data_file.as_ref().expect("Confirmed path exists");
    let file = File::open(path)?;

    let mut reader = BufReader::new(file);
    let mut line_buffer = String::new();
    let mut data_chunks = Vec::new();

    // Read file data and split into chunks of the requested size
    let mut current_chunk = Vec::new();
    loop {
        line_buffer.clear();
        let bytes_read = reader.read_line(&mut line_buffer)?;
        if bytes_read == 0 {
            break; // EOF
        }

        let line_bytes = line_buffer.as_bytes();
        for &byte in line_bytes {
            current_chunk.push(byte);
            if current_chunk.len() >= bench.request_size as usize {
                data_chunks.push(current_chunk.clone());
                current_chunk.clear();
            }
        }
    }

    // Add any remaining data as final chunk, padding if necessary
    if !current_chunk.is_empty() {
        current_chunk.resize(bench.request_size as usize, 0);
        data_chunks.push(current_chunk);
    }

    // If we don't have enough chunks, cycle through them
    if data_chunks.is_empty() {
        bail!("Data file {} contains no data", path.to_string_lossy());
    }

    // Send benchmark requests using file data
    let tx_delay = bench.send_delay.map(Duration::from_millis);
    for i in 0..request_count {
        if let Some(t) = tx_delay {
            std::thread::sleep(t);
        }

        let data = data_chunks[i % data_chunks.len()].clone();
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
