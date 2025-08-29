use crate::{
    args::GetArgs,
    input_io::Input,
    message::{dispatch_counted_batches, prelude::*},
};

use anyhow::Result;

use super::{print_and_filter_err, CommandHandler, Res, MAX_ID_BATCH_SIZE};

/// Get command handler.
///
/// Gets store entries using their primary or custom key. Sends all as a single batch for CLI data,
/// or per-line for IO data with individual error reporting.
pub fn get(handler: &CommandHandler, res: &Res, get_args: GetArgs) -> Result<()> {
    if let Some(data) = get_args.data {
        // Handle CLI data - batch processing with fail-fast error handling
        let keys = match KeySet::parse_with_type(&data, get_args.key_bytes) {
            Ok(keys) => keys,
            Err(err) => {
                eprintln!("Warning: Could not parse line: {}", err);
                return Ok(());
            }
        };

        let req = Request::Get(GetReq {
            keys,
            content_mode: get_args.content,
        });
        handler.send(req, &res)?;
    } else {
        // Handle IO data - per-line processing with individual error reporting
        // Note that with input will try and use stdio if the path is None, therefore covering the case where both the
        // data and file are None
        let input = match Input::try_new(get_args.input.as_ref()) {
            Ok(input) => input,
            Err(err) => {
                eprintln!("Could not read input: {}", err);
                return Ok(());
            }
        };

        if get_args.key_bytes {
            // Process custom keys - one per line
            let _ = dispatch_counted_batches(
                input
                    .into_custom_key_iter()
                    .filter_map(print_and_filter_err),
                MAX_ID_BATCH_SIZE,
                |batch| {
                    let get_req = GetReq {
                        keys: KeySet::Custom(batch),
                        content_mode: get_args.content,
                    };
                    handler.send(Request::Get(get_req), &res)
                },
            )?;
        } else {
            // Process UIDs - one per line
            let _ = dispatch_counted_batches(
                input.into_uid_iter().filter_map(print_and_filter_err),
                MAX_ID_BATCH_SIZE,
                |batch| {
                    let get_req = GetReq {
                        keys: KeySet::Uid(batch),
                        content_mode: get_args.content,
                    };
                    handler.send(Request::Get(get_req), &res)
                },
            )?;
        }
    }

    Ok(())
}
