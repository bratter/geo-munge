# Development Notes for GM-Proximity

## Architecture

- The revised design has the processing all done in a long-lived server process that accepts messages from a client.
- The client (or an SDK) passes all data through to the server in shorter-lived processes.
- The client does any form of data formatting or processing, but is still limited in the forms that it can accept.
- These input formats are probably some form of CSV with WKT/WKB geometry and/or ND JSON - both of which are line-oriented.
  - Starting hypothesis is to just use ND JSON as it will likely be easier to use overall.
- Should consider being able to load/store into SQLITE on the server.
  - New items would then be indexed in memory then also stored in SQLITE.
  - Would write to SQ LITE in an IO thread based on a queue, so didn't hold up processing.
- We need to be able to index.

## General Improvements

- Depending on differences in required dependencies, consider a separate binary for client and server.
- Need to think through better handling for IO loop.
- Start with just spherical coordinates, consider generalizing later.

## Messages

### Client → Server

- Insert: Insert a single item into the quad tree.
- Stats: Get some statistics about the current quad tree. Includes PK setting, count, bounding box.
- Reset: Empty the quad tree.
- Load: Load a quad tree directly from a file (if the SQLITE thing is done).
- Window: Grab all shapes within a bounding box. Can take metadata filters.
  - Window could optionally group for close-together items that are deeper in a box above a certain count.
- KNN: Conduct a KNN search. Should be able to take a count, radius or bounding box, and metadata filters.
- Define Primary Key: Default is auto-increment, but can be set to a metadata field with a type. Can only be reset at start or after a flush/reset.
  - If auto increment, the numeric key will be sent back, or the added key.
  - The quad tree internal will only use u64 keys, but _may_ choose to use provided u64s
- Bounding Box: Define the bounding box. Consider supporting multiple bounding boxes if non-contiguous.
- Delete: For completeness, should be able to delete a node. Will not support updating.

### Server → Client

- Success: Acknowledge a client message and provide a success result for those not returning data.
- Error: If the request was an error.
- List: Responses with data. Can be provided with or without metadata, and metadata can be chosen.
- Stats: Response with statistics.

## Data Integration

There are several improved requirements for data handling:

- Same ID needs to be removed from matching always.
- Need to be able to filter by metadata items.
- Likely to be two easy formats for metadata:
  - Could force all metadata to be in flat file format to make it easier to process/query.
  - Alternatively JSON would work too, with some form of JSON query language.
- Metadata storage could be managed in several ways:
  - Loading all in memory using some form of index to easily find metadata.
  - Use SQLite. This can manage JSON querying as well as flat file querying.
- Thinking that we might want to integrate data handling.

## Quad Tree

The quad tree will need improvement.

- At least will have to put inside an RwLock as first pass.
- Consider a fine-grained locking model.
- Consider fixed memory layout for a block that contains a couple of levels, then could lock for each block.
  - If a cache line is 64 bytes, should try and size the type to a multiple of this.
  - Each entry will be either a 64-bit number or a pointer (or half that on the numbers to save space), so 8 bytes.
  - 10 entries, a pointer for overflow, and 4 child pointers gives 15 \* 8 = 120 bytes, so pretty good.
  - A two layer construct would be 5 \* 10 entries, 5 overflow pointers, and 4 \* 4 child pointers = 71 \* 8 = 568 bytes.
- Consider not using Haversine depending on the required precision, or only using it for distance output or disambiguation, not intermediate.
  - Can use Cartesian with an adjustment factor at least for rect-rect comparisons.
- No square root on square test.
- Using the quad tree should be easier, or maybe the AsGeom trait will have it handled.
- Consider the new quadtree just taking in geo::Geometry, take out the intermediate one.
- The quadtree only stores the index of the item (this may be what it does already.
- Consider mapped binary representation as an alternative data structure.
- Add an average neighbor distance method
- Is there some way for the outer level to subdivide into almost squares,
  then the locks can at lest be shareded at this level, would need logic for
  managing checks with adjacents, but inserts would always be in a single pillar

## Bench

Need some sort of benchmarking to test iterations

- Test harness in general spins up an unnamed pipe and passes it to each run fn
- Needs to baseline on no-op, just generate the data on the test client and
  pump through the io loop and measure the return trip
- Ideally need seeded values, but also needs to be super fast
- One has to be spread, but another needs to be concentrated
- Mostly write then read, but options with mixed read/write

## Back Pressure

Server:

- ReadResult::Request
  - Think we have to throttle send on the channel here, use try_send with a sync channel
  - Disable read interest on the connection, store the rejected message in the connection
  - Simpler alternative is to just use send and block the io loop
  - This may gum up outbound io, but will probably work fine as a starting point
    - Would probably have to work on a per-connection basis for simplicity
- If we do disable read interest, then will have to re-enable
  - Can use a mio::Waker that gets passed around with the channel receiver that calls wake when messages are processed
  - Then the interest in mio re-enables the read interest for the connections
  - Or use crossbeam's len() and capacity() to re-enable at the top of the loop
  - Crossbeam solution is probably the best balance
- Start with just blocking at first
- Also should manage outbound, although probably not necessary, it is always possible that a client is slow at reading
- Each connection's outbound buffer is uncapped, so should block READ interest when WRITE buffer is filled - this will at least stop unbounded growth
- This can be done by reporting Full out of push_write_queue then actioning the restult in the Pool
- Read interest can be checked for re-enabling in the write method then returned in the WriteResult

Client:

- On receiving responses, just block in processing - should only require adding a bound
- On sending things in if there are multiple, the outbound buffer is unbounded, so just stop recv manually when it is full
- Because we want a soft full, we also have to check on the outside and the inside of the try_recv loop
- If we use crossbeam, then in the client we can just check if there are any items at the top of the loop and not stage in a second buffer

