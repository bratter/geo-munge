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

## Profiling

- See the puffin crate for an example of setting up profiling
- Should we use RDTSCP as a profiling mechanism
- Need to think about the multi-threading nature of the code

## Numerical Solving

- Need to replace point-line method with latest from turf
- Should work on the line-line method
  - Are we better off thinking of the problem as maximizing the dot product between the two test points
  - Should we think of defining points along the segments as rotations around the normal (from cross product
  - Need to see if there are some early outs or other numerical simplifications that we can apply
  - Can capitalize on any segment being < Pi rad
- Can pull the line-line method from Observable - its largely working
- Likely return the distances with both
- Put these in a geodesic math module - don't call haversine
- Set up the final data structure such that it automatically switches to euclidean when it gets below a certain resolution
- WARN: The axis aligned bounding box won't work for geodesics - the geodesic line can pop out the top - need to deal with this somehow
- See the solution in the Observable notebook
- Also note that will need to assign to multiple levels on subsequent pass through the data structure

TODO: Immediate next steps:

1. Client Config
2. Done response (incl count)
3. ConnId incrementing
4. Request id from client then in preamble
5. Harmonize handlers across server/client... maybe an iterator of responses?
   Will have to be somewhat different as the use cases are different
6. Integrate stdio in client - not sure this is possible, leave as-is for now
7. Windows named pipe and anon socket for linux testing (windows testing needs to work too)
8. Back pressure
9. Use tracing for instrumentation
10. Tune io... set up the benching then run through doing io only, maybe with a short sleep for calcs

FIX:

- [x] Urgent! Running KNN on points using 42 as the target and 20 output numbers 1462, 1661 out of order - there must be something wrong, likely with rect-rect distances
      With fixed bbox logic it isn't so bad, but 1661 is still out of order
- [ ] Add prop testing for rect-rect
- [ ] Ensure rect-rect distances are working, including no overlap in the lat range
- [ ] See if it is easy to extend numerical solving to arbitrary lines
- [ ] Check what needs to happen for end-to-end knn, even if slow
- [x] Fix the radians conversion issue - work on the best place to do it and also how to handle bounding boxes, plus, if we return geojson, we should unconvert on the way out
      Radian conversion boundary should probably be anything geojson is degrees, anything geo:: is radians, then BBox has two types
      Could enforce with a Rad(f64) numeric type if we implemented it in math...
- [x] Also fix distances being returned as radians
- [x] Need to fix logic of bounding box handling and degrees vs. radians - input in client should be degrees, but everything else radians
- [x] Stack overflow - caused by points at exactly the same location - likely fix at the same time as proper iteration
- [ ] Graceful termination - Long times between queries on the REPL times out (and shouldn't) some shutdowns produce crashes rather than graceful handling, etc.
- [ ] FZF REPL routine not working on Windows
- [ ] REPL is sometimes disconnecting for unknown reasons - see if can repro
- [ ] Error printout on get in REPL - not sure what this meant
- [ ] In REPL's get, the custom key path should show when set (but this requires knowing it from stats)
- [x] In REPL, some JSON decode error seems to fail in the client side read, which might be ok, but should have better messaging (check for everything)
- [x] REPL separate threads for sending large data
- [x] Fix REPL settings and data return values throughout so that there is consistent handling of return data (id, custom id, geometry, metadata)
- [x] Work on consistency of batching across everything that needs to be batched
- [x] Settings geometries not showing current setting on list
- [x] Deal with UTF8-BOM and UTF16 natively
- [x] We have to do a better job of presenting knn results that ties in with
      the input (i.e., the line number of the input within the WHOLE query,
      and also push non-data messages to stderr not stdout
- [x] The spatial traits should be consistent in returning references or owned values, at the moment they are different
- [x] Write a debug impl for Request and Response that just reports the Vecs as the len()
- [x] Look and the response handler and tracker logic and see if this is ok
- [x] Skim all client outputs for println, and make sure eprintln's are consistent
- [x] General todo cleanup in files with lots of todos
- [x] Should some of the tracing::info be tracing::debug instead in the client
- [x] Header printouts when we are using csv printing as an optional flag, fully embedded non-csv responses, non quote escaped response option
- [x] Fix output formats - maybe with a comma modifier in clap - the required data can go the server, but the format can go in the tracker data
- [x] Check that the results of a call with data none piped into a get call returns the right thing
- [x] Proper knn id outputs for all response data types
- [ ] Overall architecture review and eliminating unused components
- [ ] Combine the two benchmarking binaries
- [ ] Improve structure, naming, and arguments (refs vs owned) for math in spatial
- [ ] Use the debug-print feature in the numerical solvers, fix up the warnings, perhaps implement a golden section search, test better?
- [ ] Tighten up deletes and race conditions in the geostore
- [ ] Should we implement error code reporting back to the client rather than strings?
- [ ] Revisit the codec for transport and data typing in the server overall to minimize copying/allocations and not use geojson, this will also help sizing for batches
- [ ] The responses could also include a done flag in the header to avoid sending done responses
- [ ] Should window be able to output anything that is contained when an arbitrary shape is passed rather than a bbox?
- [ ] In all error handling, should we be more circumspect about what is anon-recoverable vs. otherwise. E.g., any fail in client handler kills the client, but should it?
- [ ] Look at the data transport, not a great idea to convert from geojson on the server side
- [x] Should check file args in convert and make sure they are the same as here
- [x] Should CLI args have short in general, or are we better off just making them long?
- [ ] Work on the custom key setup, including typing (maybe not just raw binary) and returning in ProximityResult
- [ ] Revisit the distance presentation and perhaps use a radians-wrapping newtype and add an option on the client
- [x] Should KeyGenerator::GeoJsonId be a custom key type instead of a numeric uid? Also enable it in the Args, maybe be changing the args to be a
- [ ] Do we want to add just a distance request type that somehow takes pairs of ids? Would need to think through the request and CLI data format
- [ ] Add ability to auto-parse fields in csv and kml convert processing
  - Auto-parse should/could try to parse as xml/html table in other locations other than descriptions... or have a setting... run in test mode
  - Can tweak the HTML parsing to cover more use cases
  - Kml could also recurse into placemark elements
- [ ] Consider adding a strict mode to convert parsing and rendering that errors (or at least notifies) if fields are missing/extra fields, etc.
- [ ] Deal with coordinate systems in convert
- [ ] Go through and add more documentation where required
- [ ] Submit issue to Turf regarding the intersection check in https://github.com/Turfjs/turf/blob/v7.2.0/packages/turf-nearest-point-on-line/index.ts#L192
- [ ] Look at anyhow's ensure macro for if ... bail!

FIX: Next round

- If we don't get rid of bincode, then check the positioning in the Cargo.toml heirarchy
- Think about changing the names of the encode and decode methods on IoCodec - they can probably just be encode and decode
- Don't forget to deal with the bench command - I think now is the time to consolidate them
- When we are processing geojson in the client, I wonder if we should allow our parsed feature to convert the geojson id to our byte key, or a numeric key byte key would be a challenge - if we basically say the id is the ProvidedKey, then can ignore or not ignore, but at least it is there and will error appropriately and be available, might be confusing that it is not used
- Id handling...
  - Will also think about if we want to preserve the content of the geojson id
  - On the client, json conversion could still take a key from the id field if its a number. See if we are going to allow that...
  - When we change the protocol, the parsed feature equivalent in the protocol will have to deal with the geojson conversion, which means it comes out of geo/feature on the server - the test in feature would probably be best shifting out too
  - We may allow the client to select how to define the provided key, this might also include a client-side json-pointer extraction, or maybe all custom keys should be client extracted? Let's dig into this once we have the protocol
- I don't like the way how the geostore returns records, can we have it returning features? Or at least deref to the Feature given the safety concerns
- In protocol, move some common items out from req/res into a shared module
- Once protocol is done, see if we want to pull some of the types used in geostore out, probably not going to be worth it, but think about it
- Tweaks to bench:
  - Move the impls to their own files
  - Do we want to reuse the file for a spec to try and get max speed? Or be able to configure it
- Do we want to do anything with the server settings - set the server and client config in the spec for the benchmarks? Specifically the buffer sizes?
- Can we remove bench client from proximity-client/arc/args.rs?
- Need to dump settings and results as json to analyze output
- Need to include profiling, probably using RDTSCP
