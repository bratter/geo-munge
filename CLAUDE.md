# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build and Development Commands

This is a Rust workspace with multiple binaries and libraries. Use standard Cargo commands:

- `cargo build` - Build all workspace members
- `cargo build --release` - Release build 
- `cargo test` - Run tests across the workspace
- `cargo run --bin <binary>` - Run specific binary (proximity, gm-proximity, meta, convert, bench)
- `cargo clippy` - Lint code
- `cargo fmt` - Format code

Individual crates can be built/tested with `cargo build -p <crate-name>`.

## Architecture Overview

This is a collection of geospatial data processing utilities built as a Rust workspace:

### Core Libraries
- **spatial/**: Spatial indexing library with quadtree implementation and distance calculations (Haversine, Euclidean). Provides `BasicQuadTree`, distance algorithms, and spatial index traits.
- **geolib/**: Format conversion library supporting CSV, GeoJSON, KML, and Shapefile formats with unified interfaces.

### Binaries
- **proximity/**: Single-threaded and multi-threaded proximity search using quadtrees. Processes CSV input/output for k-nearest neighbor and radius searches.
- **gm-proximity/**: Client-server proximity service with Unix sockets (Linux) or TCP (Windows). Supports concurrent access, REPL interface, and persistent spatial indexes.
- **meta/**: Metadata extraction utility for various geospatial formats.
- **convert/**: Format conversion between GIS formats (in development).
- **bench/**: Benchmarking utilities for performance testing.

### Key Design Patterns
- **Client-Server Architecture**: `gm-proximity` uses a server that maintains spatial indexes in memory with clients connecting via sockets
- **Spatial Indexing**: Quadtree-based indexing with configurable depth and child limits for efficient proximity searches
- **Format Abstraction**: Unified interfaces in `geolib` for reading/writing different geospatial formats
- **Concurrent Access**: Thread-safe spatial data structures using `Arc`, `RwLock`, and `DashMap` for multi-client access
- **Distance Calculations**: Support for both Haversine (spherical Earth) and Euclidean distance calculations

### Data Flow
1. Data ingestion through format-specific readers in `geolib`
2. Spatial indexing using quadtree structures from `spatial` library
3. Proximity queries (k-NN, radius search) against indexed data
4. Results output in various formats

The codebase focuses on performance optimization for large-scale geospatial proximity analysis with plans for SIMD vectorization and GPU offloading.