use crate::{args::DeleteArgs, input_io::Input, message::prelude::*};

use anyhow::Result;

use super::CommandHandler;

/// Delete command handler.
///
/// Removes items from the server based on their primary or custom key.
pub fn delete(handler: &mut CommandHandler, del_args: DeleteArgs) -> Result<()> {
    if let Some(data) = del_args.data {
        let keys = match KeySet::parse_with_type(&data, del_args.key_bytes) {
            Ok(keys) => keys,
            Err(err) => {
                eprintln!("Warning: Could not parse line: {}", err);
                return Ok(());
            }
        };
        handler.send(Request::Delete(keys))?;
    } else {
        // Handle IO data - per-line processing with individual error reporting
        // Note that with input will try and use stdio if the path is None, therefore covering the case where both the
        // data and file are None
        let input = match Input::try_new(del_args.file.as_ref()) {
            Ok(input) => input,
            Err(err) => {
                eprintln!("Could not read input: {}", err);
                return Ok(());
            }
        };

        if del_args.key_bytes {
            // Process custom keys - one per line
            for key_result in input.into_custom_key_iter() {
                match key_result {
                    Ok(key) => {
                        let keyset = KeySet::Custom(vec![key]);
                        // TODO: Batching
                        handler.send(Request::Delete(keyset))?;
                    }
                    Err(err) => eprintln!("Warning: Could not parse line: {}", err),
                }
            }
        } else {
            // Process UIDs - one per line
            for key_result in input.into_uid_iter() {
                match key_result {
                    Ok(uid) => {
                        let keyset = KeySet::Uid(vec![uid]);
                        // TODO: Batching
                        handler.send(Request::Delete(keyset))?;
                    }
                    Err(err) => eprintln!("Warning: Could not parse line: {}", err),
                }
            }
        }
    }

    Ok(())
}
