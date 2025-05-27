use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use crossbeam::channel;

use crate::{
    args::{Bench, BenchClient, ClientCommand},
    ctrlc::RunToken,
};

/// TODO: Get this working properly
/// - More parameters
/// - Should we take a config file also so less CLI params?
/// - Instrument!
/// - Consider pushing a channel to the
/// - Work out why the server is closing the client connection
/// - Run multiple iterations potentially with different params
pub fn run(bench: Bench, running: RunToken) -> Result<()> {
    let (ready_send, ready_recv) = channel::bounded(0);
    let server_context = crate::Context {
        config: crate::server::Config::default(),
        ready: Some(ready_send),
        running: running.clone(),
    };
    let client_context = crate::Context {
        config: crate::client::Config::default(),
        ready: None,
        running: running.clone(),
    };
    let bench_client = BenchClient {
        total_data: bench.total_data.unwrap_or(1),
        request_size: bench.request_size.unwrap_or(256),
        response_size: bench.response_size.unwrap_or(256),
        response_ratio: bench.response_ratio.unwrap_or(1),
        handle_delay: bench.handle_delay,
        send_delay: bench.send_delay,
        receive_delay: bench.receive_delay,
    };

    let total_data = bench_client.total_data * 1024 * 1024;
    let request_count = total_data / (bench_client.request_size as usize);
    if request_count == 0 {
        bail!(
            "total data of {}mb too low for request size {}b",
            bench_client.total_data,
            bench_client.request_size
        );
    }

    println!(
        "Starting bench: {}Mb total data; processing time {:?}ms/req\nRequests: {}; {}b/req; {:?}ms delay",
        bench_client.total_data, bench_client.handle_delay, request_count, bench_client.request_size, bench_client.send_delay
    );
    println!(
        "Responses: {}; {}b/res; {:?}ms delay",
        request_count * bench_client.response_ratio as usize,
        bench_client.response_size,
        bench_client.receive_delay
    );

    // Start the server and wait for it to come online before spawning the client
    let server_handle = std::thread::spawn(|| crate::server::run(server_context));
    ready_recv.recv_timeout(Duration::from_millis(50))?;
    tracing::info!("Server online, starting bench client");

    let start = Instant::now();
    let client_handle = std::thread::spawn(|| {
        crate::client::run(ClientCommand::Bench(bench_client), client_context)
    });

    // TODO: Do something with all these results
    let _ = client_handle.join().expect("Join failed");
    let total_duration = start.elapsed();

    running.shutdown();
    let _ = server_handle.join().expect("Join failed");

    println!("Bench complete: {}ms", total_duration.as_millis());
    Ok(())
}
