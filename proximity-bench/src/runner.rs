//! Unified benchmark runner for all benchmark types.
//!
//! Provides a single interface for executing different types of benchmarks
//! with consistent metrics collection and result reporting.

use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use crossbeam::channel;
use network::signals::RunToken;
use proximity_client::{args::BenchClient, config::Config as ClientConfig};
use proximity_ipc::Context;
use proximity_server::config::Config as ServerConfig;

use crate::{
    builder::{DataSetBuilder, TempDataSet},
    specs::{BenchSpec, BenchmarkType},
};

/// Results from a single benchmark run.
#[derive(Debug, Clone)]
pub struct BenchResult {
    /// Name of the benchmark specification.
    pub spec_name: String,
    /// Run index (0-based).
    pub run_index: usize,
    /// Type of benchmark that was executed.
    pub benchmark_type: String,
    /// Total duration of the benchmark.
    pub duration: Duration,
    /// Amount of data processed (estimated based on spec).
    pub data_processed_mb: f64,
    /// Number of operations/requests completed.
    pub operations_completed: usize,
    /// Throughput in megabytes per second.
    pub throughput_mbps: f64,
    /// Operations per second.
    pub ops_per_second: f64,
    /// Whether the benchmark completed successfully.
    pub success: bool,
    /// Optional error message if benchmark failed.
    pub error_message: Option<String>,
}

impl BenchResult {
    /// Create a new successful benchmark result.
    pub fn success(
        spec: &BenchSpec,
        run_index: usize,
        duration: Duration,
        operations_completed: usize,
    ) -> Self {
        let data_processed_mb = spec.data_size.estimated_megabytes() as f64;
        let duration_secs = duration.as_secs_f64();

        let throughput_mbps = if duration_secs > 0.0 {
            data_processed_mb / duration_secs
        } else {
            0.0
        };

        let ops_per_second = if duration_secs > 0.0 {
            operations_completed as f64 / duration_secs
        } else {
            0.0
        };

        Self {
            spec_name: spec.name.clone(),
            run_index,
            benchmark_type: spec.benchmark_type.to_string(),
            duration,
            data_processed_mb,
            operations_completed,
            throughput_mbps,
            ops_per_second,
            success: true,
            error_message: None,
        }
    }

    /// Create a new failed benchmark result.
    pub fn failure(spec: &BenchSpec, run_index: usize, error: String) -> Self {
        Self {
            spec_name: spec.name.clone(),
            run_index,
            benchmark_type: spec.benchmark_type.to_string(),
            duration: Duration::ZERO,
            data_processed_mb: 0.0,
            operations_completed: 0,
            throughput_mbps: 0.0,
            ops_per_second: 0.0,
            success: false,
            error_message: Some(error),
        }
    }
}

/// Unified benchmark runner that can execute all benchmark types.
pub struct BenchRunner {
    builder: DataSetBuilder,
    running: RunToken,
}

impl BenchRunner {
    /// Create a new benchmark runner.
    pub fn new(running: RunToken) -> Result<Self> {
        let builder = DataSetBuilder::new()?;
        Ok(Self { builder, running })
    }

    /// Run a single benchmark specification with all its iterations.
    /// Returns results for all runs of the spec.
    pub fn run_spec(&mut self, spec: BenchSpec) -> Result<Vec<BenchResult>> {
        tracing::info!("Starting benchmark: {}", spec.name);

        // Start the proximity server once for all runs of this spec
        let (ready_send, ready_recv) = channel::bounded(0);
        let server_context = Context {
            config: ServerConfig::default(),
            ready: Some(ready_send),
            running: self.running.clone(),
        };

        let server_handle = std::thread::spawn(|| proximity_server::run(server_context));

        // Wait for server to be ready
        if ready_recv.recv_timeout(Duration::from_millis(100)).is_err() {
            tracing::error!("Server failed to start within timeout");
            self.running.shutdown();
            let _ = server_handle.join();
            bail!("Server failed to start");
        }

        tracing::debug!("Proximity server started for benchmark: {}", spec.name);

        // Run all iterations against the same server
        let mut results = Vec::new();
        for run_index in 0..spec.runs {
            if !self.running.is_running() {
                tracing::warn!("Benchmark cancelled by user");
                break;
            }

            tracing::debug!(
                "Running {} iteration {}/{}",
                spec.name,
                run_index + 1,
                spec.runs
            );

            let result = self.run_single(&spec, run_index);
            match &result {
                Ok(bench_result) if bench_result.success => {
                    tracing::info!(
                        "{} run {} completed: {:.2}ms, {:.2} MB/s",
                        spec.name,
                        run_index,
                        bench_result.duration.as_millis(),
                        bench_result.throughput_mbps
                    );
                }
                Ok(bench_result) => {
                    tracing::error!(
                        "{} run {} failed: {}",
                        spec.name,
                        run_index,
                        bench_result
                            .error_message
                            .as_deref()
                            .unwrap_or("Unknown error")
                    );
                }
                Err(e) => {
                    tracing::error!("{} run {} errored: {}", spec.name, run_index, e);
                }
            }

            results.push(result?);
        }

        // Shutdown the server after all runs complete
        tracing::debug!(
            "Shutting down proximity server for benchmark: {}",
            spec.name
        );
        self.running.shutdown();
        let _ = server_handle.join();

        tracing::info!("Completed benchmark: {}", spec.name);
        Ok(results)
    }

    /// Run a single benchmark iteration.
    fn run_single(&mut self, spec: &BenchSpec, run_index: usize) -> Result<BenchResult> {
        // Build any required temporary files
        let temp_data = self.builder.build_for_run(spec, run_index)?;

        // Execute the appropriate benchmark type
        match &spec.benchmark_type {
            BenchmarkType::Ipc { .. } => self.run_ipc_benchmark(spec, run_index),
            BenchmarkType::Protocol { .. } => {
                self.run_protocol_benchmark(spec, run_index, temp_data.unwrap())
            }
            BenchmarkType::Disk { .. } => self.run_disk_benchmark(spec, run_index),
            BenchmarkType::ProximitySearch { .. } => {
                self.run_proximity_benchmark(spec, run_index, temp_data.unwrap())
            }
        }
    }

    /// Run IPC throughput benchmark.
    fn run_ipc_benchmark(&self, spec: &BenchSpec, run_index: usize) -> Result<BenchResult> {
        let BenchmarkType::Ipc {
            request_size,
            response_size,
            response_ratio,
            handle_delay,
            send_delay,
            receive_delay,
        } = &spec.benchmark_type
        else {
            return Err(anyhow!("Invalid benchmark type for IPC benchmark"));
        };

        // Calculate total data and request count
        let total_data_mb = spec.data_size.estimated_megabytes();
        let total_data_bytes = total_data_mb * 1024 * 1024;
        let request_count = total_data_bytes / (*request_size as usize);

        if request_count == 0 {
            return Ok(BenchResult::failure(
                spec,
                run_index,
                format!(
                    "Total data of {}MB too low for request size {}B",
                    total_data_mb, request_size
                ),
            ));
        }

        tracing::debug!(
            "IPC benchmark run {} - {}MB total data, {} requests of {}B each",
            run_index,
            total_data_mb,
            request_count,
            request_size
        );

        // Create client context
        let client_context = Context {
            config: ClientConfig::default(),
            ready: None,
            // Needs a new RunToken as we don't want to shut the system down when a single client finishes
            running: RunToken::new(),
        };

        let bench_client = BenchClient {
            total_data: total_data_mb,
            request_size: *request_size,
            response_size: *response_size,
            response_ratio: *response_ratio,
            handle_delay: *handle_delay,
            send_delay: *send_delay,
            receive_delay: *receive_delay,
        };

        // Run the benchmark
        let client_handle = std::thread::spawn(move || {
            let start = Instant::now();
            if let Err(err) = proximity_client::run(
                proximity_client::args::ClientCommand::Bench(bench_client),
                client_context,
            ) {
                bail!(err)
            } else {
                Ok(start.elapsed())
            }
        });

        // Wait for client to complete and measure duration
        let client_result = client_handle.join();

        // Check results
        match client_result {
            Ok(Ok(duration)) => {
                tracing::debug!(
                    "IPC benchmark run {} completed in {}ms",
                    run_index,
                    duration.as_millis()
                );
                Ok(BenchResult::success(
                    spec,
                    run_index,
                    duration,
                    request_count,
                ))
            }
            Ok(Err(e)) => Ok(BenchResult::failure(
                spec,
                run_index,
                format!("Client error: {}", e),
            )),
            Err(_) => Ok(BenchResult::failure(
                spec,
                run_index,
                "Client thread panicked".to_string(),
            )),
        }
    }

    /// Run protocol encoding/decoding benchmark.
    fn run_protocol_benchmark(
        &self,
        spec: &BenchSpec,
        run_index: usize,
        _temp_data: TempDataSet,
    ) -> Result<BenchResult> {
        let start = Instant::now();

        // TODO: Implement protocol encoding/decoding tests
        // Load data from temp_data.data_path()
        // Encode/decode in a loop to test serialization performance
        std::thread::sleep(Duration::from_millis(50));

        let duration = start.elapsed();
        Ok(BenchResult::success(spec, run_index, duration, 500))
    }

    /// Run disk I/O throughput benchmark.
    fn run_disk_benchmark(&self, spec: &BenchSpec, run_index: usize) -> Result<BenchResult> {
        let start = Instant::now();

        // TODO: Implement disk I/O throughput tests
        std::thread::sleep(Duration::from_millis(75));

        let duration = start.elapsed();
        Ok(BenchResult::success(spec, run_index, duration, 750))
    }

    /// Run proximity search benchmark.
    fn run_proximity_benchmark(
        &self,
        spec: &BenchSpec,
        run_index: usize,
        _temp_data: TempDataSet,
    ) -> Result<BenchResult> {
        let start = Instant::now();

        // TODO: Implement proximity search benchmarks
        // Use gm-proximity client/server with data from temp_data.data_path()
        // Run queries from temp_data.query_path()
        std::thread::sleep(Duration::from_millis(200));

        let duration = start.elapsed();
        Ok(BenchResult::success(spec, run_index, duration, 100))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::specs::{BenchmarkType, DataSize};

    fn create_test_spec(name: &str, benchmark_type: BenchmarkType) -> BenchSpec {
        BenchSpec {
            name: name.to_string(),
            runs: 2,
            seed: Some(42),
            data_size: DataSize::RowCount(100),
            bbox: None,
            description: None,
            benchmark_type,
        }
    }

    #[test]
    fn test_bench_result_success() {
        let spec = create_test_spec(
            "test",
            BenchmarkType::Ipc {
                request_size: 256,
                response_size: 256,
                response_ratio: 1,
                handle_delay: None,
                send_delay: None,
                receive_delay: None,
            },
        );

        let result = BenchResult::success(&spec, 0, Duration::from_millis(100), 1000);

        assert_eq!(result.spec_name, "test");
        assert_eq!(result.run_index, 0);
        assert_eq!(result.benchmark_type, "IPC");
        assert!(result.success);
        assert!(result.ops_per_second > 0.0);
    }

    #[test]
    fn test_bench_result_failure() {
        let spec = create_test_spec("test", BenchmarkType::Protocol {});
        let result = BenchResult::failure(&spec, 1, "Test error".to_string());

        assert_eq!(result.spec_name, "test");
        assert_eq!(result.run_index, 1);
        assert_eq!(result.benchmark_type, "Protocol");
        assert!(!result.success);
        assert_eq!(result.error_message, Some("Test error".to_string()));
    }

    #[test]
    fn test_runner_creation() {
        let running = RunToken::new();
        let runner = BenchRunner::new(running);
        assert!(runner.is_ok());
    }
}
