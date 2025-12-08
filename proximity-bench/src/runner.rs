//! Unified benchmark runner for all benchmark types.
//!
//! Provides a single interface for executing different types of benchmarks
//! with consistent metrics collection and result reporting.

use std::{
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{bail, Result};
use crossbeam::channel;
use network::signals::RunToken;
use proximity_client::{args::BenchClient, config::Config as ClientConfig};
use proximity_ipc::Context;
use proximity_server::config::Config as ServerConfig;

use crate::{
    builder::{DataSetBuilder, TempDataSet},
    specs::{BenchSpec, BenchmarkType, Disk, Ipc, Protocol, ProximitySearch},
};

macro_rules! fail {
    ($ri:expr, $msg:literal $(,)?) => {
        return $crate::runner::BenchResult::failure($ri, format!($msg))
    };
    ($ri:expr, $fmt:literal, $($arg:tt)*) => {
        return $crate::runner::BenchResult::failure($ri, format!($fmt, $($arg)*))
    };
}

macro_rules! fail_if {
    ($cond:expr, $ri:expr, $msg:literal $(,)?) => {
        if $cond {
            fail!($ri, $msg)
        }
    };
    ($cond:expr, $ri:expr, $fmt:literal, $($arg:tt)*) => {
        if $cond {
            fail!($ri, $fmt, $($arg)*)
        }
    };
}

/// Results from a single benchmark run.
#[derive(Debug, Clone)]
pub enum BenchResult {
    Success(Success),
    Failure(Failure),
}

#[derive(Debug, Clone)]
pub struct Success {
    /// Run index (0-based).
    pub run_index: usize,
    /// Total duration of the benchmark.
    pub duration: Duration,
    /// Amount of data processed (estimated based on spec).
    pub data_processed_mb: usize,
    /// Number of operations/requests completed.
    pub operations_completed: usize,
}

#[derive(Debug, Clone)]
pub struct Failure {
    /// Run index (0-based).
    pub run_index: usize,
    /// Error message from the run.
    pub error_message: String,
}

impl BenchResult {
    /// Create a new successful benchmark result.
    pub fn success(
        run_index: usize,
        data_processed_mb: usize,
        operations_completed: usize,
        duration: Duration,
    ) -> Self {
        Self::Success(Success {
            run_index,
            duration,
            data_processed_mb,
            operations_completed,
        })
    }

    /// Create a new failed benchmark result.
    pub fn failure(run_index: usize, error_message: String) -> Self {
        Self::Failure(Failure {
            run_index,
            error_message,
        })
    }
}

impl Success {
    /// Throughput in megabytes per second.
    pub fn throughput_mbps(&self) -> f64 {
        let duration_secs = self.duration.as_secs_f64();

        if duration_secs > 0.0 {
            self.data_processed_mb as f64 / duration_secs
        } else {
            0.0
        }
    }

    /// Operations per second.
    pub fn ops_per_sec(&self) -> f64 {
        let duration_secs = self.duration.as_secs_f64();

        if duration_secs > 0.0 {
            self.operations_completed as f64 / duration_secs
        } else {
            0.0
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
    pub fn run_spec(&mut self, spec: &BenchSpec) -> Result<Vec<BenchResult>> {
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

            let result = self.run_single(&spec, run_index)?;

            // Print immediate feedback for this run
            match &result {
                BenchResult::Success(success) => {
                    println!(
                        "✅ Run {}: {:.2}ms | {:.2} MB/s | {:.0} ops/s",
                        success.run_index,
                        success.duration.as_millis(),
                        success.throughput_mbps(),
                        success.ops_per_sec()
                    );
                }
                BenchResult::Failure(failure) => {
                    println!(
                        "❌ Run {}: FAILED - {}",
                        failure.run_index, failure.error_message
                    );
                }
            }

            results.push(result);
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
        // TODO: Do we want the speed benefits of one for all, the randomness benefits of one each, or both?
        // Consider moving this based on the desired bahavior
        let temp_data = self.builder.build_for_run(spec, run_index)?;

        // Execute the appropriate benchmark type
        match &spec.benchmark_type {
            BenchmarkType::Ipc(ipc) => Ok(self.run_ipc_benchmark(run_index, ipc, &temp_data)),
            BenchmarkType::Disk(disk) => Ok(self.run_disk_benchmark(run_index, disk, &temp_data)),
            BenchmarkType::Protocol(proto) => {
                self.run_protocol_benchmark(run_index, proto, &temp_data)
            }
            BenchmarkType::ProximitySearch(proximity) => {
                self.run_proximity_benchmark(run_index, proximity, &temp_data)
            }
        }
    }

    /// Run IPC throughput benchmark.
    fn run_ipc_benchmark(
        &self,
        run_index: usize,
        ipc: &Ipc,
        temp_data: &TempDataSet,
    ) -> BenchResult {
        // Calculate total data and request count
        let total_data_mb = temp_data.data_size.estimated_megabytes();
        let total_data_bytes = total_data_mb * 1024 * 1024;
        let op_count = total_data_bytes / (ipc.request_size as usize);

        fail_if!(
            op_count == 0,
            run_index,
            "Total data of {}B too low for request size {}B",
            total_data_bytes,
            ipc.request_size
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
            request_size: ipc.request_size,
            response_size: ipc.response_size,
            response_ratio: ipc.response_ratio,
            handle_delay: ipc.handle_delay,
            send_delay: ipc.send_delay,
            receive_delay: ipc.receive_delay,
            data_file: None,
        };

        // Run the benchmark
        let client_result = std::thread::spawn(move || {
            let start = Instant::now();
            if let Err(err) = proximity_client::run(
                proximity_client::args::ClientCommand::Bench(bench_client),
                client_context,
            ) {
                bail!(err)
            } else {
                Ok(start.elapsed())
            }
        })
        .join();

        // Check results
        match client_result {
            Ok(Ok(duration)) => BenchResult::success(run_index, total_data_mb, op_count, duration),
            Ok(Err(e)) => fail!(run_index, "Client error: {}", e),
            Err(_) => fail!(run_index, "Client thread panicked"),
        }
    }

    /// Run disk I/O throughput benchmark.
    fn run_disk_benchmark(
        &self,
        run_index: usize,
        disk: &Disk,
        temp_data: &TempDataSet,
    ) -> BenchResult {
        // Calculate total data and request count
        let total_data_mb = temp_data.data_size.estimated_megabytes();
        let total_data_bytes = total_data_mb * 1024 * 1024;
        let op_count = total_data_bytes / (disk.request_size as usize);

        fail_if!(
            op_count == 0,
            run_index,
            "Total data of {}MB too low for request size {}B",
            total_data_mb,
            disk.response_size
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
            request_size: disk.request_size,
            response_size: disk.response_size,
            response_ratio: disk.response_ratio,
            handle_delay: disk.handle_delay,
            send_delay: disk.send_delay,
            receive_delay: disk.receive_delay,
            data_file: temp_data.data_path().map(Path::to_path_buf),
        };

        // Run the benchmark
        let client_result = std::thread::spawn(move || {
            let start = Instant::now();
            if let Err(err) = proximity_client::run(
                proximity_client::args::ClientCommand::Bench(bench_client),
                client_context,
            ) {
                bail!(err)
            } else {
                Ok(start.elapsed())
            }
        })
        .join();

        // Check results
        match client_result {
            Ok(Ok(duration)) => BenchResult::success(run_index, total_data_mb, op_count, duration),
            Ok(Err(e)) => fail!(run_index, "Client error: {}", e),
            Err(_) => fail!(run_index, "Client thread panicked"),
        }
    }

    /// Run protocol encoding/decoding benchmark.
    fn run_protocol_benchmark(
        &self,
        run_index: usize,
        _protocol: &Protocol,
        _temp_data: &TempDataSet,
    ) -> Result<BenchResult> {
        let start = Instant::now();

        // TODO: Implement protocol encoding/decoding tests
        // Load data from temp_data.data_path()
        // Encode/decode in a loop to test serialization performance
        std::thread::sleep(Duration::from_millis(50));

        let duration = start.elapsed();
        Ok(BenchResult::success(run_index, 500, 10, duration))
    }

    /// Run proximity search benchmark.
    fn run_proximity_benchmark(
        &self,
        run_index: usize,
        _proximity: &ProximitySearch,
        _temp_data: &TempDataSet,
    ) -> Result<BenchResult> {
        let start = Instant::now();

        // TODO: Implement proximity search benchmarks
        // Use gm-proximity client/server with data from temp_data.data_path()
        // Run queries from temp_data.query_path()
        std::thread::sleep(Duration::from_millis(200));

        let duration = start.elapsed();
        Ok(BenchResult::success(run_index, 100, 5, duration))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::specs::{BenchmarkType, DataSize, Ipc};

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
        // TODO: Exercise the spec in a test
        let _spec = create_test_spec(
            "test",
            BenchmarkType::Ipc(Ipc {
                request_size: 256,
                response_size: 256,
                response_ratio: 1,
                handle_delay: None,
                send_delay: None,
                receive_delay: None,
            }),
        );

        let result = BenchResult::success(0, 1000, 100, Duration::from_millis(100));

        match result {
            BenchResult::Success(success) => {
                assert_eq!(success.run_index, 0);
                assert!(success.ops_per_sec() > 0.0);
            }
            BenchResult::Failure(_) => panic!("Should be success"),
        }
    }

    #[test]
    fn test_bench_result_failure() {
        let result = BenchResult::failure(1, "Test error".to_string());

        match result {
            BenchResult::Success(_) => panic!("Should be failure"),
            BenchResult::Failure(failure) => {
                assert_eq!(failure.run_index, 1);
                assert_eq!(failure.error_message, "Test error");
            }
        }
    }

    #[test]
    fn test_runner_creation() {
        let running = RunToken::new();
        let runner = BenchRunner::new(running);
        assert!(runner.is_ok());
    }
}
