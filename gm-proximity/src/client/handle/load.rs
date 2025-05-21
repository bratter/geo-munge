use std::path::PathBuf;

use anyhow::Result;

use crate::message::prelude::*;

use super::{CommandHandler, Input};

/// Load command handler
pub fn load(handler: &mut CommandHandler, file: Option<PathBuf>) -> Result<()> {
    // TODO: This might be more efficient if we don't transport this via lines, but grab using byte separators directly,
    // then have the request take a slice instead of a Vec
    // TODO: Batch lines
    for (n, data_result) in Input::try_new(file)?.into_data_stream_iter().enumerate() {
        match data_result {
            Ok(data) => {
                // TODO: Do we want to break out of the loop on an error, or just report on these - think they are
                // mostly stream faliures, if this is the case then aborting is correct
                handler.send(Request::Insert(data))?;
            }
            Err(err) => eprintln!("Could not read line {}: {}", n, err),
        }
    }

    Ok(())
}
