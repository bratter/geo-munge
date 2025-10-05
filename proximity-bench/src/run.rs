use anyhow::Result;
use network::signals::RunToken;

use crate::{
    args::Args,
    runner::BenchRunner,
    specs::{BenchSpec, BenchmarkType, DataSize},
};

/// Run benchmarks using the new unified BenchRunner framework.
/// Converts legacy Args into BenchSpec and uses BenchRunner for execution.
pub fn run(bench: Args, running: RunToken) -> Result<()> {
    // Convert legacy Args into new BenchSpec format
    let spec = create_spec_from_args(&bench);

    // Create and run the benchmark
    let mut runner = BenchRunner::new(running)?;
    let results = runner.run_spec(spec)?;

    // Print results summary
    print_results_summary(&results);

    Ok(())
}

/// Convert legacy Args into a BenchSpec for backward compatibility.
fn create_spec_from_args(args: &Args) -> BenchSpec {
    BenchSpec {
        name: "legacy_ipc_benchmark".to_string(),
        // FIX: The extra fields here should be filled out
        runs: 10,
        seed: None,
        data_size: DataSize::Megabytes(args.total_data.unwrap_or(1)),
        bbox: None,
        description: Some("Legacy IPC benchmark converted from Args".to_string()),
        benchmark_type: BenchmarkType::Ipc {
            request_size: args.request_size.unwrap_or(256),
            response_size: args.response_size.unwrap_or(256),
            response_ratio: args.response_ratio.unwrap_or(1),
            handle_delay: args.handle_delay,
            send_delay: args.send_delay,
            receive_delay: args.receive_delay,
        },
    }
}

/// Print a summary of benchmark results.
fn print_results_summary(results: &[crate::runner::BenchResult]) {
    if results.is_empty() {
        println!("No benchmark results to display");
        return;
    }

    println!("\n=== Benchmark Results Summary ===");

    for result in results {
        if result.success {
            println!(
                "✅ {} (run {}): {:.2}ms | {:.2} MB/s | {:.0} ops/s | {} ops",
                result.spec_name,
                result.run_index,
                result.duration.as_millis(),
                result.throughput_mbps,
                result.ops_per_second,
                result.operations_completed
            );
        } else {
            println!(
                "❌ {} (run {}): FAILED - {}",
                result.spec_name,
                result.run_index,
                result.error_message.as_deref().unwrap_or("Unknown error")
            );
        }
    }

    // Calculate and display aggregate statistics for successful runs
    let successful_results: Vec<_> = results.iter().filter(|r| r.success).collect();
    if successful_results.len() > 1 {
        let avg_duration = successful_results
            .iter()
            .map(|r| r.duration.as_millis())
            .sum::<u128>() as f64
            / successful_results.len() as f64;
        let avg_throughput = successful_results
            .iter()
            .map(|r| r.throughput_mbps)
            .sum::<f64>()
            / successful_results.len() as f64;
        let avg_ops_per_sec = successful_results
            .iter()
            .map(|r| r.ops_per_second)
            .sum::<f64>()
            / successful_results.len() as f64;

        println!(
            "\n📊 Averages across {} successful runs:",
            successful_results.len()
        );
        println!(
            "   Duration: {:.2}ms | Throughput: {:.2} MB/s | Ops/sec: {:.0}",
            avg_duration, avg_throughput, avg_ops_per_sec
        );
    }

    println!("=================================\n");
}
