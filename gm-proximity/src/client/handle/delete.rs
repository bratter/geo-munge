use anyhow::Result;
use protocol::prelude::*;

use crate::{args::DeleteArgs, input_io::Input, message::dispatch_counted_batches};

use super::{print_and_filter_err, CommandHandler, Res, MAX_ID_BATCH_SIZE};

/// Delete command handler.
///
/// Removes items from the server based on their primary or custom key.
pub fn delete(handler: &CommandHandler, res: &Res, del_args: DeleteArgs) -> Result<()> {
    if let Some(data) = del_args.data {
        let keys = match KeySet::parse_with_type(&data, del_args.key_bytes) {
            Ok(keys) => keys,
            Err(err) => {
                eprintln!("Warning: Could not parse line: {}", err);
                return Ok(());
            }
        };
        handler.send(Request::Delete(keys), &res)?;
    } else {
        // Handle IO data - per-line processing with individual error reporting
        // Note that with input will try and use stdio if the path is None, therefore covering the case where both the
        // data and file are None
        let input = match Input::try_new(del_args.input.as_ref()) {
            Ok(input) => input,
            Err(err) => {
                eprintln!("Could not read input: {}", err);
                return Ok(());
            }
        };

        if del_args.key_bytes {
            // Process custom keys with batching
            let _ = dispatch_counted_batches(
                input
                    .into_custom_key_iter()
                    .filter_map(print_and_filter_err),
                MAX_ID_BATCH_SIZE,
                |batch| handler.send(Request::Delete(KeySet::Custom(batch)), &res),
            )?;
        } else {
            // Process UIDs with batching
            let _ = dispatch_counted_batches(
                input.into_uid_iter().filter_map(print_and_filter_err),
                MAX_ID_BATCH_SIZE,
                |batch| handler.send(Request::Delete(KeySet::Uid(batch)), &res),
            )?;
        }
    }

    Ok(())
}
