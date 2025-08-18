use std::path::PathBuf;

use anyhow::Result;

use crate::{input_io::Input, message::prelude::*};

use super::CommandHandler;

// TODO: Move these to a more central location if batching required elsewhere and we use the same stats
// TODO: Is it possible to have this 64kb batch size? How do we measure outside of the bincode serialization?
//const MAX_BATCH_BYTES: usize = 64 * 1024;
const MAX_BATCH_COUNT: usize = 100;

/// Load command handler.
///
/// Breaks when a send fails as these will be terminal errors, but WARN only on individual line errors as these could be
/// recoverable.
pub fn load(handler: &mut CommandHandler, file: Option<PathBuf>) -> Result<()> {
    let mut feature_stream = Vec::with_capacity(MAX_BATCH_COUNT);
    let mut batch_count = 0;

    // TODO: After doing all the data structure work, revist this to see if we can make the whole IPC pipeline more
    // effcient, specifically less copying and conversion
    for feature in Input::try_new(file)?.into_feature_iter() {
        match feature {
            // TODO: Consider introducing a __gmLineNumber member in the feature's properties when an appropriate
            // setting is provided on load, or some other way of using line numbers as keys explicitly
            Ok((_, feature)) => {
                if batch_count >= MAX_BATCH_COUNT {
                    let batch =
                        std::mem::replace(&mut feature_stream, Vec::with_capacity(MAX_BATCH_COUNT));
                    handler.send(Request::Insert(batch))?;
                    batch_count = 0;
                }

                feature_stream.push(feature);
                batch_count += 1;
            }
            // TODO: Harmonize interim error reporting, and decide if this is the best way
            Err(err) => eprintln!("{}", err),
        }
    }

    // Do a final flush
    if feature_stream.len() > 0 {
        handler.send(Request::Insert(feature_stream))?;
    }

    Ok(())
}
