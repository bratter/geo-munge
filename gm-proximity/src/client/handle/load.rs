use std::path::PathBuf;

use anyhow::Result;

use crate::{input_io::Input, message::prelude::*};

use super::{CommandHandler, MAX_BATCH_BYTES, MAX_FEATURE_COUNT};

/// Load command handler.
///
/// Breaks when a send fails as these will be terminal errors, but WARN only on individual line errors as these could be
/// recoverable.
pub fn load(handler: &mut CommandHandler, file: Option<PathBuf>) -> Result<()> {
    let mut feature_buffer = Vec::with_capacity(MAX_FEATURE_COUNT);
    let mut batch_bytes = 0;

    for feature in Input::try_new(file)?.into_feature_iter() {
        match feature {
            Ok((text_bytes, feature)) => {
                batch_bytes += text_bytes;
                if batch_bytes >= MAX_BATCH_BYTES && !feature_buffer.is_empty() {
                    let batch = std::mem::replace(
                        &mut feature_buffer,
                        Vec::with_capacity(MAX_FEATURE_COUNT),
                    );
                    handler.send(Request::Insert(batch))?;
                    batch_bytes = 0;
                }

                feature_buffer.push(feature);
            }
            Err(err) => eprintln!("Warning: Could not parse line: {}", err),
        }
    }

    // Do a final flush
    if feature_buffer.len() > 0 {
        handler.send(Request::Insert(feature_buffer))?;
    }

    Ok(())
}
