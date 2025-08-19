use anyhow::Result;

use crate::{args::KnnArgs, input_io::Input, message::prelude::*};

use super::CommandHandler;

/// Knn command handler.
pub fn knn(handler: &mut CommandHandler, knn_args: KnnArgs) -> Result<()> {
    if let Some(raw_data) = knn_args.data {
        // Handle CLI data - batch processing with fail-fast error handling
        let find_data = match handle_cli_data(raw_data, knn_args.key_uid, knn_args.key_bytes) {
            Ok(find_data) => find_data,
            Err(err) => {
                eprintln!("Warning: Could not parse data: {}", err);
                return Ok(());
            }
        };

        let knn_req = KnnReq {
            k: knn_args.k,
            r: knn_args.r,
            content_mode: knn_args.content,
            data: find_data,
        };

        handler.send(Request::Knn(knn_req))?;
    } else {
        // Handle IO data - per-line processing with individual error reporting
        handle_io_data(handler, &knn_args)?;
    }

    Ok(())
}

fn handle_cli_data(data: String, key_uid: bool, key_bytes: bool) -> Result<FindData> {
    let use_keys = key_uid || key_bytes;

    if use_keys {
        let keys = KeySet::parse_with_type(&data, key_bytes);
        Ok(FindData::Keys(keys?))
    } else {
        let features: Result<Vec<_>, _> = data.lines().map(|l| l.parse::<Feature>()).collect();
        Ok(FindData::Features(features?))
    }
}

// TODO: Batching
fn handle_io_data(handler: &mut CommandHandler, knn_args: &KnnArgs) -> Result<()> {
    let input = match Input::try_new(knn_args.file.as_ref()) {
        Ok(input) => input,
        Err(err) => {
            eprintln!("Could not read input: {}", err);
            return Ok(());
        }
    };

    match (knn_args.key_uid, knn_args.key_bytes) {
        (true, true) => unreachable!("key_uid and key_bytes are mutually exclusive"),
        (true, false) => {
            // Process UIDs - one per line
            for key_result in input.into_uid_iter() {
                match key_result {
                    Ok(uid) => {
                        let keyset = KeySet::Uid(vec![uid]);
                        let knn_req = KnnReq {
                            k: knn_args.k,
                            r: knn_args.r,
                            content_mode: knn_args.content,
                            data: FindData::Keys(keyset),
                        };
                        handler.send(Request::Knn(knn_req))?;
                    }
                    Err(err) => eprintln!("Warning: Could not parse line: {}", err),
                }
            }
        }
        (false, true) => {
            // Process custom keys - one per line
            for key_result in input.into_custom_key_iter() {
                match key_result {
                    Ok(key) => {
                        let keyset = KeySet::Custom(vec![key]);
                        let knn_req = KnnReq {
                            k: knn_args.k,
                            r: knn_args.r,
                            content_mode: knn_args.content,
                            data: FindData::Keys(keyset),
                        };
                        handler.send(Request::Knn(knn_req))?;
                    }
                    Err(err) => eprintln!("Warning: Could not parse line: {}", err),
                }
            }
        }
        (false, false) => {
            // Process features - one per line
            for feature_result in input.into_feature_iter() {
                match feature_result {
                    Ok((_, f)) => {
                        let knn_req = KnnReq {
                            k: knn_args.k,
                            r: knn_args.r,
                            content_mode: knn_args.content,
                            data: FindData::Features(vec![f]),
                        };
                        handler.send(Request::Knn(knn_req))?;
                    }
                    Err(err) => eprintln!("Warning: Could not parse line: {}", err),
                }
            }
        }
    }

    Ok(())
}
