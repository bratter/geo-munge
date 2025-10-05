//! Benchmark specification types and definitions.
//!
//! Provides unified specification structures for all benchmark types with common
//! parameters for consistent execution and result comparison.

use protocol::request::DegreeBbox;
use serde::{Deserialize, Serialize};

/// Complete benchmark specification with common parameters and benchmark-specific configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchSpec {
    /// Unique name for this benchmark specification.
    pub name: String,

    /// Number of times to run this benchmark for statistical analysis.
    pub runs: usize,

    /// Optional seed for reproducible random data generation.
    pub seed: Option<u64>,

    /// Size specification for data generation or transfer.
    pub data_size: DataSize,

    /// Optional bounding box for spatial operations (defaults to world bounds if needed).
    pub bbox: Option<DegreeBbox>,

    /// Optional description for documentation.
    pub description: Option<String>,

    /// Benchmark-specific configuration.
    #[serde(flatten)]
    pub benchmark_type: BenchmarkType,
}

/// Data size specification - can be either megabytes for throughput tests or row count for algorithmic tests.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum DataSize {
    /// Size specified in megabytes (for throughput-focused benchmarks).
    Megabytes(usize),
    /// Size specified by number of rows/records (for algorithm-focused benchmarks).
    RowCount(usize),
}

impl DataSize {
    /// Estimate the approximate number of rows for megabyte specifications.
    /// Uses ~150 bytes per GeoJSON point feature as rough estimate.
    pub fn estimated_rows(&self) -> usize {
        match self {
            DataSize::Megabytes(mb) => (mb * 1024 * 1024) / 150,
            DataSize::RowCount(rows) => *rows,
        }
    }

    /// Estimate the approximate megabytes for row count specifications.
    /// Uses ~150 bytes per GeoJSON point feature as rough estimate.
    pub fn estimated_megabytes(&self) -> usize {
        match self {
            DataSize::Megabytes(mb) => *mb,
            DataSize::RowCount(rows) => (rows * 150) / (1024 * 1024).max(1),
        }
    }
}

/// Benchmark type with specific configuration for each type of test.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BenchmarkType {
    /// IPC throughput benchmark - tests network/socket performance.
    Ipc {
        /// Size of each request in bytes.
        request_size: u32,
        /// Size of each response in bytes.
        response_size: u32,
        /// Number of responses per request.
        response_ratio: u32,
        /// Optional delay in milliseconds for each response generation.
        handle_delay: Option<u64>,
        /// Optional delay in milliseconds between sending requests.
        send_delay: Option<u64>,
        /// Optional delay in milliseconds between receiving responses.
        receive_delay: Option<u64>,
    },

    /// Disk I/O throughput benchmark - tests file system performance.
    Disk {
        /// Size of each request in bytes.
        request_size: u32,
        /// Size of each response in bytes.
        response_size: u32,
        /// Number of responses per request.
        response_ratio: u32,
        /// Optional delay in milliseconds for each response generation.
        handle_delay: Option<u64>,
        /// Optional delay in milliseconds between sending requests.
        send_delay: Option<u64>,
        /// Optional delay in milliseconds between receiving responses.
        receive_delay: Option<u64>,
    },

    /// Protocol encoding/decoding benchmark - tests serialization performance.
    Protocol {},

    /// Proximity search benchmark - tests geospatial query performance.
    ProximitySearch {
        /// Number of query points to test.
        query_count: usize,
        /// Number of nearest neighbors to find (k in k-NN).
        k: usize,
        /// Optional maximum search radius in degrees.
        radius: Option<f64>,
    },
}

impl BenchmarkType {
    /// Returns true if this benchmark type requires data file generation.
    pub fn requires_data_generation(&self) -> bool {
        matches!(
            self,
            BenchmarkType::Protocol {} | BenchmarkType::ProximitySearch { .. }
        )
    }

    /// Returns true if this benchmark type requires query file generation.
    pub fn requires_query_generation(&self) -> bool {
        matches!(self, BenchmarkType::ProximitySearch { .. })
    }
}

impl std::fmt::Display for BenchmarkType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Ipc { .. } => "IPC",
            Self::Disk { .. } => "Disk",
            Self::Protocol { .. } => "Protocol",
            Self::ProximitySearch { .. } => "ProximitySearch",
        };

        s.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_size_conversions() {
        let mb_spec = DataSize::Megabytes(10);
        let row_spec = DataSize::RowCount(1000);

        assert_eq!(mb_spec.estimated_megabytes(), 10);
        assert!(mb_spec.estimated_rows() > 60000); // ~10MB / 150 bytes

        assert_eq!(row_spec.estimated_rows(), 1000);
        assert_eq!(row_spec.estimated_megabytes(), 0); // Small size rounds to 0
    }

    #[test]
    fn test_benchmark_type_flags() {
        let ipc = BenchmarkType::Ipc {
            request_size: 256,
            response_size: 256,
            response_ratio: 1,
            handle_delay: None,
            send_delay: None,
            receive_delay: None,
        };

        let proximity = BenchmarkType::ProximitySearch {
            query_count: 100,
            k: 10,
            radius: None,
        };

        assert!(!ipc.requires_data_generation());
        assert!(!ipc.requires_query_generation());

        assert!(proximity.requires_data_generation());
        assert!(proximity.requires_query_generation());
    }

    #[test]
    fn test_spec_serialization() {
        let spec = BenchSpec {
            name: "test_spec".to_string(),
            runs: 5,
            seed: Some(42),
            data_size: DataSize::RowCount(1000),
            bbox: None,
            description: Some("Test description".to_string()),
            benchmark_type: BenchmarkType::Protocol {},
        };

        let json = serde_json::to_string_pretty(&spec).unwrap();
        let deserialized: BenchSpec = serde_json::from_str(&json).unwrap();

        assert_eq!(spec.name, deserialized.name);
        assert_eq!(spec.runs, deserialized.runs);
        assert_eq!(spec.seed, deserialized.seed);
    }
}

