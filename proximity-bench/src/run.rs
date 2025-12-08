use std::{fs, path::PathBuf};

use anyhow::{bail, Context, Result};
use network::signals::RunToken;

use crate::{
    args::Args,
    runner::{BenchResult, BenchRunner},
    specs::BenchSpec,
};

/// Run benchmarks using the new unified BenchRunner framework.
/// Loads benchmark specification from JSON file.
pub fn run(args: Args, running: RunToken) -> Result<()> {
    // Load benchmark specification from JSON
    let spec = load_spec(&args.spec_file)?;

    print_spec_info(&spec);

    // Create and run the benchmark
    let mut runner = BenchRunner::new(running)?;
    let results = runner.run_spec(&spec)?;

    print_results_summary(&spec, &results);

    Ok(())
}

/// Load a benchmark specification from JSON file with fallback to specs directory.
fn load_spec(spec_file: &str) -> Result<BenchSpec> {
    // Try the provided path first
    let mut path = PathBuf::from(spec_file);

    if !path.exists() {
        // If not found, try in the specs directory
        // TODO: What to do in non-dev environments? Use the non-compiled version?
        path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("specs")
            .join(spec_file);

        if !path.exists() {
            bail!(
                "Spec file '{}' not found. Tried:\n  - {}\n  - specs/{}",
                spec_file,
                spec_file,
                spec_file
            );
        }
    }

    let content = fs::read_to_string(&path)?;
    let spec: BenchSpec = serde_json::from_str(&content).with_context(|| {
        format!(
            "Failure deserializing bench spec {}",
            path.to_string_lossy()
        )
    })?;

    Ok(spec)
}

fn print_spec_info(spec: &BenchSpec) {
    println!("📋 Benchmark Specification");
    println!("   Name: {}", spec.name);
    if let Some(description) = &spec.description {
        println!("   Description: {}", description);
    }
    println!("   Runs: {}", spec.runs);
    println!();
}

/// Print a summary of benchmark results.
fn print_results_summary(spec: &BenchSpec, results: &[BenchResult]) {
    if results.is_empty() {
        println!("\n📊 No benchmark results to display");
        return;
    }

    // Collect and sort successful results by duration
    let mut successful: Vec<_> = results
        .iter()
        .filter_map(|r| match r {
            BenchResult::Success(success) => Some(success),
            BenchResult::Failure(_) => None,
        })
        .collect();
    if successful.is_empty() {
        println!("\n📊 No successful runs to aggregate");
        return;
    }

    successful.sort_by(|a, b| a.duration.partial_cmp(&b.duration).unwrap());
    let count = successful.len();

    // Calculate all statistics in a single pass
    let mut sum_duration_secs = 0.0;
    let mut sum_throughput = 0.0;
    let mut sum_ops = 0.0;

    for result in &successful {
        let duration_secs = result.duration.as_secs_f64();
        sum_duration_secs += duration_secs;
        sum_throughput += result.throughput_mbps();
        sum_ops += result.ops_per_sec();
    }

    // Calculate averages
    let avg_duration = sum_duration_secs / count as f64;
    let avg_throughput = sum_throughput / count as f64;
    let avg_ops = sum_ops / count as f64;

    // Calculate medians (already sorted by duration)
    let median_idx = count / 2;
    let medians = if count % 2 == 0 {
        let duration = (successful[median_idx - 1].duration.as_secs_f64()
            + successful[median_idx].duration.as_secs_f64())
            / 2.0;
        let throughput = (successful[median_idx - 1].throughput_mbps()
            + successful[median_idx].throughput_mbps())
            / 2.0;
        let ops =
            (successful[median_idx - 1].ops_per_sec() + successful[median_idx].ops_per_sec()) / 2.0;

        (duration, throughput, ops)
    } else {
        (
            successful[median_idx].duration.as_secs_f64(),
            successful[median_idx].throughput_mbps(),
            successful[median_idx].ops_per_sec(),
        )
    };

    // Best/worst from sorted array
    let best = successful[0];
    let worst = successful[count - 1];

    // Print table
    println!(
        "\n📊 Aggregates for spec: {} ({}/{} successes)",
        spec.name,
        count,
        results.len()
    );
    println!();
    println!("Metric                   Best       Worst         Avg      Median");
    println!("─────────────────────────────────────────────────────────────────");
    println!(
        "Duration (ms)     {:11.2} {:11.2} {:11.2} {:11.2}",
        best.duration.as_secs_f64() * 1000.0,
        worst.duration.as_secs_f64() * 1000.0,
        avg_duration * 1000.0,
        medians.0 * 1000.0
    );
    println!(
        "Throughput (MB/s) {:11.2} {:11.2} {:11.2} {:11.2}",
        best.throughput_mbps(),
        worst.throughput_mbps(),
        avg_throughput,
        medians.1
    );
    println!(
        "Ops/sec           {:11.0} {:11.0} {:11.0} {:11.0}",
        best.ops_per_sec(),
        worst.ops_per_sec(),
        avg_ops,
        medians.2
    );
}
