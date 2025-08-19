use crate::{args::GetArgs, input_io::Input, message::prelude::*};

use anyhow::Result;

use super::CommandHandler;

/// Get command handler.
///
/// Gets store entries using their primary or custom key. Sends all as a single batch for CLI data,
/// or per-line for IO data with individual error reporting.
pub fn get(handler: &mut CommandHandler, get_args: GetArgs) -> Result<()> {
    if let Some(data) = get_args.data {
        // Handle CLI data - batch processing with fail-fast error handling
        let keys = match KeySet::parse_with_type(&data, get_args.key_bytes) {
            Ok(keys) => keys,
            Err(err) => {
                eprintln!("Warning: Could not parse line: {}", err);
                return Ok(());
            }
        };

        handler.send(Request::Get(GetReq {
            keys,
            content_mode: get_args.content,
        }))?;
    } else {
        // Handle IO data - per-line processing with individual error reporting
        // Note that with input will try and use stdio if the path is None, therefore covering the case where both the
        // data and file are None
        let input = match Input::try_new(get_args.file.as_ref()) {
            Ok(input) => input,
            Err(err) => {
                eprintln!("Could not read input: {}", err);
                return Ok(());
            }
        };

        if get_args.key_bytes {
            // Process custom keys - one per line
            for key_result in input.into_custom_key_iter() {
                match key_result {
                    Ok(key) => {
                        let keyset = KeySet::Custom(vec![key]);
                        let get_req = GetReq {
                            keys: keyset,
                            content_mode: get_args.content,
                        };
                        // TODO: Batching
                        handler.send(Request::Get(get_req))?;
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
                        let get_req = GetReq {
                            keys: keyset,
                            content_mode: get_args.content,
                        };
                        // TODO: Batching
                        handler.send(Request::Get(get_req))?;
                    }
                    Err(err) => eprintln!("Warning: Could not parse line: {}", err),
                }
            }
        }
    }

    Ok(())
}
