# Architecture Review - Rust Geospatial Processing Workspace

This document captures an architecture review discussion for the geo-munge Rust workspace, focusing on crate organization and module structure for a geospatial data processing system.

## System Overview

The system is designed for geospatial data processing with specific proximity calculation workflows (not a general-purpose GIS system). Currently undergoing a rewrite with some legacy code present.

### Design Goals

- **Flexibility and performance** - working version first, optimizations (SIMD/GPU) later
- **Separation of concerns** that doesn't harm future optimization potential
- **Minimize 3rd party dependencies** and ringfence dependency proliferation
- **Cross-platform compatibility** (nix, windows, potentially WASM)

### User Personas

- **Business Analysts**: Client-oriented tools should be usable by BAs
- **Developers**: Underlying libraries only used by developers

### Main User-Facing Functions

1. **Conversion**: CLI binary for GIS format conversion and metadata reading (`convert` crate)
2. **Proximity client**: CLI binary connecting to server, handling user I/O (`gm-proximity` crate)
3. **Other clients**: Future TUI/GUI/WASM clients (not yet implemented)
4. **Proximity server**: In-memory database accepting requests, coordinating GIS processing (`gm-proximity` crate)
5. **Bench**: Internal binary for performance benchmarking

### Current Crates

- **spatial**: Core algorithm library
- **geolib**: Legacy code with format conversion logic and quadtree wrappers
- **gm-proximity**: Contains both client and server
- **meta**: Legacy user-facing binary
- **convert**: Format conversion
- **bench**: Benchmarking utilities
- **proximity**: Legacy single-threaded tool

## Current Architecture Assessment

### ✅ What's Working Well

- Clear workspace separation between core libraries and binaries
- Dependency management properly centralized in workspace
- Clean spatial algorithms in the `spatial` crate with minimal dependencies
- Server-client architecture in `gm-proximity` is well-suited for the use case

### ❌ Major Issues Identified

#### 1. Crate Boundary Violations

- **`geolib`** has mixed responsibilities: format I/O + legacy quadtree wrappers + business logic
- **`gm-proximity`** contains both client AND server code (should be separate)
- **Dependency proliferation** is already happening (gm-proximity has 14+ deps)

#### 2. Legacy Code Technical Debt

- **Two quadtree implementations** (legacy in geolib + new in spatial)
- **Format handling scattered** across multiple modules
- **`meta` crate** appears to duplicate functionality

## Proposed Architecture

### Core Libraries (Developer-Facing)

#### 1. `spatial` ✅ (Keep as-is, enhance)

```
spatial/
├── distance/     # Haversine, Euclidean, etc.
├── indexes/      # BasicQuadTree, future SIMD variants
├── math/         # Earth calculations, projections
└── traits/       # SpatialIndex, Distance traits
```

- **Role**: Pure algorithms, cross-platform compatible
- **Dependencies**: Minimal (geo, anyhow only)
- **Future**: SIMD/GPU optimization happens here

#### 2. `geoformats` (Rename from `geolib`)

```
geoformats/
├── csv/         # CSV reader/writer
├── geojson/     # GeoJSON handling
├── shapefile/   # Shapefile handling
├── kml/         # KML handling
├── traits/      # FormatReader, FormatWriter
└── metadata/    # Format metadata extraction
```

- **Role**: Pure I/O, format conversion only
- **Dependencies**: Format-specific crates (csv, geojson, etc.)
- **Remove**: All quadtree/spatial logic

#### 3. `geoprotocol` (Extract from gm-proximity)

```
geoprotocol/
├── request.rs   # Request types with Encode/Decode
├── response.rs  # Response types with Encode/Decode
└── lib.rs       # Protocol definitions
```

- **Role**: Client-server communication protocol
- **Dependencies**: bincode, geo, anyhow
- **Benefits**: Clean boundary between client and server

### Applications (Business Analyst-Facing)

#### 4. `geo-convert` (Rename from `convert`)

- **Role**: CLI for format conversion + metadata
- **Dependencies**: geoformats, clap
- **Merge**: Absorb `meta` functionality here
- **Binary name**: `gm-convert`

#### 5. `proximity-server` (Extract from gm-proximity)

- **Role**: Network server + in-memory geo database
- **Dependencies**: geoprotocol, spatial, networking libs
- **Platform**: Handle Unix sockets vs TCP differences
- **Binary name**: `gm-server`

#### 6. `proximity-client` (Extract from gm-proximity)

- **Role**: CLI client for server interaction + standalone mode
- **Dependencies**: geoprotocol, geoformats, clap
- **Binary name**: `gm-client`
- **Modes**: Both `--server` (networked) and `--standalone` (direct processing)

### Development Tools

- **`bench`** ✅ (Keep as-is)

## Key Architecture Decisions

### Binary Naming Strategy

Use `[[bin]]` sections in Cargo.toml for clean separation:

```toml
# proximity-client/Cargo.toml
[package]
name = "proximity-client"

[[bin]]
name = "gm-client"
path = "src/main.rs"
```

**Benefits**:

- Crate names stay descriptive for developers (`proximity-client`)
- Binary names stay user-friendly (`gm-client`)
- Follows Rust ecosystem patterns

### Threading Architecture

**Thread management should live in applications, NOT in core libraries:**

```
spatial/indexes → Thread-safe data structures (Arc<RwLock>, etc.)
proximity-server → Thread pool management, request dispatching
proximity-client → Rayon/thread pool for batch processing (standalone mode)
```

### Protocol Types & CLI Parsing

**Use type translation at the boundary rather than dual traits:**

```rust
// In proximity-client only
#[derive(Parser)]
struct KnnArgs {
    #[arg(short, long)]
    k: usize,
    // ... other CLI-specific args
}

impl From<KnnArgs> for geoprotocol::KnnReq {
    fn from(args: KnnArgs) -> Self {
        // Translate CLI args to protocol types
    }
}
```

**Benefits**:

- Single source of truth for protocol types
- CLI concerns stay in client
- Clean separation of responsibilities

### Trait Organization

**Keep traits with their domains, not in separate `traits/` modules:**

```rust
// Good - domain-organized
spatial/
├── distance/
│   ├── mod.rs          # pub trait Distance + implementations
│   ├── haversine.rs
│   └── euclidean.rs
├── indexes/
│   ├── mod.rs          # pub trait SpatialIndex + BasicQuadTree
│   └── basic_quadtree.rs
└── lib.rs              # Re-exports for prelude
```

This follows standard Rust patterns and keeps related code together.

### Handler Architecture

**Handlers serve completely different purposes and should remain separate:**

**Client Handlers**:

- I/O orchestration (file reading, CLI parsing)
- Request preparation (CLI args → protocol types)
- Batching strategy for performance
- Error resilience (per-line error handling)

**Server Handlers**:

- Algorithm execution (actual proximity searches)
- Business logic (geometry processing, distance calculations)
- Response formatting (search results → response types)
- Memory management (result batching, iterator optimization)

No shared handler code - the protocol types are the interface between them.

### GeoStore Placement

**Decision**: Keep GeoStore in `proximity-server` for now.

**Rationale**:

- **YAGNI**: Only used there currently, no clear second use case
- **Size**: ~300 lines isn't massive yet
- **Coupling**: Tightly integrated with server's threading/response patterns
- **Simplicity**: One less crate boundary to manage during rewrite

**Future Migration Path**:

```
Phase 1: Keep GeoStore in proximity-server
Phase 2: If it grows >1000 lines OR gets a second user → extract to geostore crate
Phase 3: If multiple storage backends needed → extract with traits
```

**Why NOT in `spatial`**: GeoStore is a database layer with dependencies (dashmap, fxhash, JSON parsing) and threading concerns that would pollute the pure algorithms crate.

### Dependency Isolation Strategy

**Ring-fence heavy dependencies by crate:**

- **Networking**: Only in server/client binaries
- **Format libraries**: Only in `geoformats`
- **UI libraries**: Only in specific client implementations
- **Core**: Keep `spatial` dependency-light for WASM compatibility

## Migration Strategy

### Phase 1: Clean Boundaries

1. Create `geoprotocol` crate with shared protocol types
2. Split `gm-proximity` into `proximity-server` and `proximity-client`
3. Remove quadtree logic from `geolib`

### Phase 2: Consolidation

1. Rename `geolib` → `geoformats`, remove non-format code
2. Merge `meta` functionality into `geo-convert`
3. Update all dependency declarations

### Phase 3: Future-Proofing

1. Add trait abstractions for pluggable components
2. Prepare `spatial` for SIMD variants
3. Add WASM compatibility gates

## Final Architecture Summary

**Core Libraries:**

- `spatial` - Pure algorithms + thread-safe indexes
- `geoformats` - I/O and format conversion only
- `geoprotocol` - Client-server communication types

**Applications:**

- `geo-convert` → `gm-convert` - Format conversion + metadata
- `proximity-server` → `gm-server` - Service daemon + GeoStore
- `proximity-client` → `gm-client` - Unified client (networked + standalone modes)

**Remove:**

- `proximity` (merge functionality into client)
- `meta` (merge into convert)

This architecture provides clear separation between pure algorithms, I/O handling, business logic, and user interfaces while maintaining flexibility for future clients (TUI, GUI, WASM) and optimizations.

## Key Insights from Review

1. **Handlers are not generic** - client and server handlers serve completely different purposes and should remain separate
2. **GeoStore is substantial** - it's a full in-memory database implementation that could be extracted but doesn't need to be yet
3. **Protocol boundary is key** - clean separation between client/server via shared protocol types
4. **Traits belong with domains** - not in separate modules unless truly cross-cutting
5. **Threading at application level** - core libraries provide thread-safe primitives, applications manage concurrency
6. **YAGNI for extractions** - don't extract until there's clear benefit (second user or size threshold)

---

## Follow-up Discussion: Data Formats and Wire Protocol

### Data Pipeline Analysis

After the initial architecture review, we conducted a deeper analysis of the data formats and transport mechanisms used throughout the system. The current pipeline has several inefficiencies:

**Current Data Flow:**

```
Input:  NDJSON -> Client (parse/validate) -> stringify -> bincode -> Server (parse) -> Storage
Output: Server -> stringify -> bincode -> Client -> format -> NDJSON
```

**Issues Identified:**

- Double encoding/parsing (JSON → string → bincode, then bincode → string → JSON)
- String transport inefficiency (JSON strings are larger than binary)
- Validation happens twice (client validates, server re-parses)
- Protocol tied to Rust (bincode limits future clients)

### Recommended Data Format Changes

#### Wire Protocol: MessagePack over Bincode

**Decision**: Replace bincode with MessagePack for client-server communication.

**Rationale**:

- **Language agnostic** - enables future Python/JavaScript clients
- **Still compact binary** - ~20% larger than bincode, much smaller than JSON
- **Good performance** - 2-3x slower than bincode, but still fast
- **Schema flexible** - handles missing fields better than bincode
- **Debug tooling** - can inspect with standard MessagePack tools

#### Geometry Transport: Custom Binary Format

**Decision**: Replace bincoded GeoJSON strings with custom serialized `geo_types::Geometry`.

**Implementation**:

```rust
// Direct serialization of geo_types, avoiding intermediate formats
impl Serialize for geo::Geometry<f64> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            geo::Geometry::Point(p) => (0u8, p.x(), p.y()).serialize(serializer),
            geo::Geometry::LineString(ls) => (1u8, &ls.0).serialize(serializer),
            // ... other variants
        }
    }
}
```

**Benefits**:

- **~4x faster** than current (eliminate JSON string round-trips)
- **~2x smaller** wire format (binary vs JSON strings)
- **Language agnostic** - creates de facto binary geometry specification
- **Future flexible** - could swap to WKB if standardization needed

#### Properties: Direct MessagePack Encoding

**Decision**: Keep `serde_json::Value` for storage, add direct MessagePack serialization.

**Implementation**:

```rust
impl Serialize for Properties {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Direct MessagePack encoding, skip JSON string step
        self.0.serialize(serializer)
    }
}
```

**Rationale**:

- **No stringify step** - direct binary encoding of properties
- **Same storage format** - still `serde_json::Value` internally for JSON pointer access
- **Efficient transport** - binary properties over wire

#### Processing Boundary: Client-Side Validation

**Decision**: Move validation and format conversion to client boundary.

**New Data Flow**:

```
Input:  NDJSON -> Client (validate/convert/radians) -> MessagePack -> Server (store directly)
Output: Server -> MessagePack -> Client (format) -> NDJSON
```

**Benefits**:

- **Single parse** - client validates once, server trusts input
- **Performance** - server focuses on algorithms, not parsing
- **Error locality** - validation errors reported at data source
- **Scalability** - parsing work distributes across clients

#### Input/Output Formats: Keep NDJSON

**Decision**: Maintain newline-delimited GeoJSON for user-facing I/O.

**Rationale**:

- **Streamable** - process GB+ files without loading into memory
- **Standardized** - GeoJSON spec compliance
- **Toolchain compatible** - works with `gm-convert`, `jq`, etc.
- **Human readable** - easy debugging and inspection
- **Format consistency** - symmetric input/output experience

### geoprotocol Crate Design

#### Dependencies

```toml
[dependencies]
anyhow = { workspace = true }          # Error handling
rmp-serde = { workspace = true }       # MessagePack serialization
geo = { workspace = true }            # Geometry types
geojson = { workspace = true }        # GeoJSON conversion utilities
serde_json = { workspace = true }     # Properties JSON parsing
```

#### Structure

```
geoprotocol/
├── protocol/     # Request/Response enums
├── types/        # Shared types (NodeId, Properties, Feature)
├── geometry/     # Custom geo_types serialization
└── lib.rs       # Re-exports and prelude
```

#### Shared Types Placement

**Decision**: Use `geoprotocol` as shared types hub.

**Types in geoprotocol**:

- `NodeId` - record identifiers used in protocol and storage
- `Properties` - JSON metadata wrapper with MessagePack serialization
- `Feature` - geometry + properties with efficient wire encoding

**Rationale**:

- **Single source of truth** for serializable types
- **Cross-client consistency** guaranteed
- **Manageable scale** - not worth separate crate yet
- **Clean boundaries** - protocol owns wire format concerns

### Cross-Language Compatibility

The custom geometry serialization creates a **language-agnostic binary specification** that can be implemented in any language:

**JavaScript Example**:

```javascript
function decodeGeometry(buffer) {
  const view = new DataView(buffer);
  const typeId = view.getUint8(0);
  // Decode based on type_id to GeoJSON format
}
```

**Python Example**:

```python
def decode_geometry(data: bytes) -> Dict[str, Any]:
    type_id = struct.unpack('B', data[0:1])[0]
    # Decode based on type_id to geometry dict
```

This approach provides immediate performance benefits for Rust while creating a path for future multi-language support.

---

## Implementation Tasks

### Protocol Foundation

- [ ] **Create geoprotocol crate with basic structure**

  - Set up `Cargo.toml` with MessagePack dependencies (`rmp-serde`, `geo`, `anyhow`, `geojson`, `serde_json`)
  - Create module structure: `protocol/`, `types/`, `geometry/`, `lib.rs`
  - Add to workspace and verify builds

- [x] **Extract shared types from gm-proximity**

  - Move `NodeId`, `CustomKey`, `KeyMode`, `ContentMode`, `DegreeBbox`
  - Move `Properties` wrapper with JSON pointer support
  - Test all existing conversions (`FromStr`, `TryFrom`, etc.) still work

- [ ] **Implement custom geometry serialization**

  - Create discriminated union format for `geo_types::Geometry`
  - Add `Serialize`/`Deserialize` implementations for MessagePack
  - Add radians/degrees conversion utilities
  - Test wire format size vs current JSON strings (expect ~50% reduction)

- [ ] **Implement efficient Properties serialization**

  - Add direct MessagePack encoding (skip JSON stringify step)
  - Preserve JSON pointer functionality for server
  - Test serialization roundtrip and performance vs current approach

- [ ] **Create unified Feature type**

  - Combine `geo_types::Geometry` + `Properties` + `NodeId`
  - Test end-to-end serialization: Feature → MessagePack → Feature
  - Measure performance improvement vs current pipeline

- [ ] **Extract protocol Request/Response enums**
  - Move all variant types from gm-proximity message module
  - Replace bincode traits with MessagePack equivalents
  - Test protocol roundtrip and measure wire format size reduction

### Client Restructure

- [ ] **Create proximity-client crate**

  - Set up new crate with `[[bin]] name = "gm-client"`
  - Copy client modules: `args/`, `input_io/`, `client/handle/`, etc.
  - Update imports to use `geoprotocol` types
  - Verify crate builds and basic CLI works

- [ ] **Implement client-side validation pipeline**

  - Create NDJSON → `geoprotocol::Feature` conversion with validation
  - Add geometry validation and radians conversion at input boundary
  - Test error handling for malformed input
  - Measure parsing performance vs server-side parsing

- [ ] **Update client handlers for new protocol**

  - Modify handlers to translate CLI args to `geoprotocol` types
  - Remove server-side parsing assumptions
  - Preserve existing batching logic and streaming
  - Test all command types work with new protocol

- [ ] **Implement response formatting**
  - Add `geoprotocol` responses → NDJSON conversion
  - Handle all content modes (full, geometry-only, properties-only)
  - Preserve CSV/JSON hybrid output options
  - Test output format consistency vs current implementation

### Server Migration

- [ ] **Create proximity-server crate**

  - Set up new crate with `[[bin]] name = "gm-server"`
  - Copy server modules: `server/`, `connection/`, etc.
  - Keep `GeoStore` in server crate
  - Update imports and verify builds

- [ ] **Update server for geoprotocol integration**

  - Replace message types with `geoprotocol` equivalents
  - Update connection handling for MessagePack vs bincode
  - Test basic request/response cycle works
  - Measure wire protocol performance vs bincode

- [ ] **Update server handlers**

  - Remove geometry parsing (expect pre-validated input)
  - Update handlers to work with `geo_types::Geometry` directly
  - Test all operations work with new data flow
  - Measure server performance improvement

- [ ] **Update GeoStore for new types**
  - Modify storage to use `geoprotocol::Feature`
  - Update indexing for pre-converted geometries
  - Adapt custom key extraction for new `Properties`
  - Test concurrent access patterns still work

### Format Library Cleanup

- [x] **Rename geolib to geoformats**

  - Update crate name in `Cargo.toml` and workspace
  - Clean module structure around pure I/O functionality
  - Remove legacy quadtree integration code
  - Test all format readers/writers still work

- [x] **Merge meta functionality into geo-convert**

  - Extract metadata logic from `meta` crate
  - Add metadata commands to `geo-convert` CLI
  - Update binary name to `gm-convert`
  - Remove `meta` crate from workspace
  - Remove `proximity` crate from workspace

### Integration and Cleanup

- [ ] **End-to-end integration testing**

  - Test complete NDJSON → client → server → client → NDJSON pipeline
  - Verify all command types work with new architecture
  - Test error handling at all boundaries
  - Compare performance metrics vs baseline

- [ ] **Performance validation**

  - Measure actual wire format size reduction (target: 60%)
  - Time complete request/response cycles (target: 40% improvement)
  - Profile server CPU usage reduction
  - Test memory usage under load

- [ ] **Update workspace configuration**

  - Remove old crate references (`gm-proximity`, `meta`, `proximity`)
  - Update shared dependency versions
  - Test `cargo build`, `cargo test`, `cargo clippy` across workspace
  - Verify cross-crate imports are correct

- [ ] **Documentation updates**
  - Update README with new binary names and architecture
  - Document new data pipeline and wire format
  - Create migration guide for existing workflows
  - Document wire format specification for future language implementations

### Success Metrics

- [ ] **Wire format 60% smaller** than JSON strings
- [ ] **Processing 40% faster** with eliminated double parsing
- [ ] **Clean crate boundaries** with single responsibilities
- [ ] **Preserved user interfaces** (same input/output formats)
- [ ] **Language-agnostic protocol** ready for future clients
