//! Dataset builder for generating temporary files for benchmarking.
//!
//! Handles creation of temporary data and query files needed for various benchmark types,
//! with automatic cleanup when datasets are dropped. Each run gets fresh, deterministic data
//! for better statistical validity.

use std::io::Write;

use anyhow::Result;
use tempfile::{NamedTempFile, TempDir};

use crate::{
    generate::PointGenerator,
    specs::{BenchSpec, BenchmarkType, DataSize, ProximitySearch},
};

/// Manages temporary files for benchmark datasets.
pub struct DataSetBuilder {
    temp_dir: TempDir,
}

impl DataSetBuilder {
    /// Create a new dataset builder with a temporary directory.
    pub fn new() -> Result<Self> {
        let temp_dir = TempDir::new()?;
        Ok(Self { temp_dir })
    }

    /// Build dataset files for a specific run of the given benchmark specification.
    /// Returns None if the benchmark type doesn't require file generation.
    /// Each run gets unique data based on the run index for better statistical analysis.
    pub fn build_for_run(&self, spec: &BenchSpec, run_index: usize) -> Result<TempDataSet> {
        let run_seed = self.calculate_run_seed(spec.seed, run_index);

        match &spec.benchmark_type {
            // These benchmark types don't require data file generation
            BenchmarkType::Ipc(_) => Ok(TempDataSet::with_size(spec.data_size)),
            // Disk and Protocol benchmarks only need data files for encoding/decoding
            &BenchmarkType::Disk(_) | BenchmarkType::Protocol(_) => {
                let data_file = self.generate_data_file(spec, run_seed)?;
                Ok(TempDataSet {
                    data_file: Some(data_file),
                    query_file: None,
                    data_size: spec.data_size,
                })
            }
            BenchmarkType::ProximitySearch(ProximitySearch { query_count, .. }) => {
                // Proximity search benchmarks need both data and query files
                let data_file = self.generate_data_file(spec, run_seed)?;
                let query_file = self.generate_query_file(spec, run_seed, *query_count)?;
                Ok(TempDataSet {
                    data_file: Some(data_file),
                    query_file: Some(query_file),
                    data_size: spec.data_size,
                })
            }
        }
    }

    /// Calculate deterministic but unique seed for each run.
    /// Uses a large prime multiplier to ensure good seed distribution.
    fn calculate_run_seed(&self, base_seed: Option<u64>, run_index: usize) -> Option<u64> {
        base_seed.map(|seed| {
            // Use a large prime to create well-distributed seeds across runs
            seed.wrapping_add((run_index as u64).wrapping_mul(1000000007))
        })
    }

    /// Generate a data file with points based on the specification.
    fn generate_data_file(&self, spec: &BenchSpec, run_seed: Option<u64>) -> Result<NamedTempFile> {
        let mut data_file = NamedTempFile::new_in(&self.temp_dir)?;

        let bbox = spec.bbox.clone().unwrap_or_default();
        let mut generator = PointGenerator::new(bbox, run_seed);

        let row_count = spec.data_size.estimated_rows();
        generator.write_points(&mut data_file, row_count)?;

        data_file.flush()?;
        Ok(data_file)
    }

    /// Generate a query file with query points based on the specification.
    /// Uses a different seed offset to ensure query points differ from data points.
    fn generate_query_file(
        &self,
        spec: &BenchSpec,
        run_seed: Option<u64>,
        query_count: usize,
    ) -> Result<NamedTempFile> {
        let mut query_file = NamedTempFile::new_in(&self.temp_dir)?;

        let bbox = spec.bbox.clone().unwrap_or_default();
        // Use a large prime offset to ensure query points are different from data points
        let query_seed = run_seed.map(|s| s.wrapping_add(7919));
        let mut generator = PointGenerator::new(bbox, query_seed);

        generator.write_points(&mut query_file, query_count)?;

        query_file.flush()?;
        Ok(query_file)
    }

    /// Get the temporary directory path for debugging or external tool access.
    pub fn temp_dir_path(&self) -> &std::path::Path {
        self.temp_dir.path()
    }
}

/// Container for temporary dataset files.
/// Files are automatically cleaned up when this struct is dropped.
pub struct TempDataSet {
    /// Optional data file (for benchmarks that need a dataset).
    pub data_file: Option<NamedTempFile>,
    /// Optional query file (for benchmarks that need query points).
    pub query_file: Option<NamedTempFile>,
    /// The size of the dataset.
    pub data_size: DataSize,
}

impl TempDataSet {
    /// Create an empty data set that only has a size.
    pub fn with_size(data_size: DataSize) -> Self {
        Self {
            data_file: None,
            query_file: None,
            data_size,
        }
    }

    /// Get the path to the data file, if it exists.
    pub fn data_path(&self) -> Option<&std::path::Path> {
        self.data_file.as_ref().map(|f| f.path())
    }

    /// Get the path to the query file, if it exists.
    pub fn query_path(&self) -> Option<&std::path::Path> {
        self.query_file.as_ref().map(|f| f.path())
    }

    /// Check if this dataset has a data file.
    pub fn has_data_file(&self) -> bool {
        self.data_file.is_some()
    }

    /// Check if this dataset has a query file.
    pub fn has_query_file(&self) -> bool {
        self.query_file.is_some()
    }
}

#[cfg(test)]
mod tests {
    use crate::specs::{Ipc, Protocol};

    use super::*;

    fn create_test_spec(benchmark_type: BenchmarkType) -> BenchSpec {
        BenchSpec {
            name: "test_spec".to_string(),
            runs: 3,
            seed: Some(42),
            data_size: DataSize::RowCount(100),
            bbox: None,
            description: None,
            benchmark_type,
        }
    }

    #[test]
    fn test_ipc_benchmark_no_files() {
        let builder = DataSetBuilder::new().unwrap();
        let spec = create_test_spec(BenchmarkType::Ipc(Ipc {
            request_size: 256,
            response_size: 256,
            response_ratio: 1,
            handle_delay: None,
            send_delay: None,
            receive_delay: None,
        }));

        let dataset = builder.build_for_run(&spec, 0).unwrap();
        assert!(dataset.has_data_file());
        assert!(dataset.has_query_file());
    }

    #[test]
    fn test_protocol_benchmark_data_file_only() {
        let builder = DataSetBuilder::new().unwrap();
        let spec = create_test_spec(BenchmarkType::Protocol(Protocol {}));

        let dataset = builder.build_for_run(&spec, 0).unwrap();

        assert!(dataset.has_data_file());
        assert!(!dataset.has_query_file());
        assert!(dataset.data_path().unwrap().exists());
    }

    #[test]
    fn test_proximity_search_both_files() {
        let builder = DataSetBuilder::new().unwrap();
        let spec = create_test_spec(BenchmarkType::ProximitySearch(ProximitySearch {
            query_count: 10,
            k: 5,
            radius: None,
        }));

        let dataset = builder.build_for_run(&spec, 0).unwrap();

        assert!(dataset.has_data_file());
        assert!(dataset.has_query_file());
        assert!(dataset.data_path().unwrap().exists());
        assert!(dataset.query_path().unwrap().exists());
    }

    #[test]
    fn test_different_data_per_run() {
        let builder = DataSetBuilder::new().unwrap();
        let spec = create_test_spec(BenchmarkType::Protocol(Protocol {}));

        let dataset1 = builder.build_for_run(&spec, 0).unwrap();
        let dataset2 = builder.build_for_run(&spec, 1).unwrap();

        let content1 = std::fs::read_to_string(dataset1.data_path().unwrap()).unwrap();
        let content2 = std::fs::read_to_string(dataset2.data_path().unwrap()).unwrap();

        // Different runs should produce different data
        assert_ne!(content1, content2);
    }

    #[test]
    fn test_reproducible_runs() {
        let builder1 = DataSetBuilder::new().unwrap();
        let builder2 = DataSetBuilder::new().unwrap();
        let spec = create_test_spec(BenchmarkType::Protocol(Protocol {}));

        let dataset1 = builder1.build_for_run(&spec, 0).unwrap();
        let dataset2 = builder2.build_for_run(&spec, 0).unwrap();

        let content1 = std::fs::read_to_string(dataset1.data_path().unwrap()).unwrap();
        let content2 = std::fs::read_to_string(dataset2.data_path().unwrap()).unwrap();

        // Same run index with same seed should produce identical data
        assert_eq!(content1, content2);
    }

    #[test]
    fn test_file_content_generation() {
        let builder = DataSetBuilder::new().unwrap();
        let spec = create_test_spec(BenchmarkType::Protocol(Protocol {}));

        let dataset = builder.build_for_run(&spec, 0).unwrap();
        let data_path = dataset.data_path().unwrap();

        let content = std::fs::read_to_string(data_path).unwrap();
        let lines: Vec<&str> = content.trim().split('\n').collect();

        // Should have 100 lines (as specified in DataSize::RowCount(100))
        assert_eq!(lines.len(), 100);

        // Each line should be valid JSON
        for line in lines {
            let _: serde_json::Value = serde_json::from_str(line).unwrap();
        }
    }
}
