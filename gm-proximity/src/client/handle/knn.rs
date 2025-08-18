use std::io::BufRead;

use anyhow::Result;

use crate::{args::KnnArgs, input_io::Input, message::prelude::*};

use super::CommandHandler;

/// Knn command handler
/// TODO: Fix this - it is a bare minimum test version
/// TODO: See notes in the load handler - improvements here will be similiar
/// TODO: After those are done, then try to simplify - e.g. abstract the 4 iterators behind an impl - we could even make
/// a string input a bufreader and just make it work as input
pub fn knn(handler: &mut CommandHandler, knn_args: KnnArgs) -> Result<()> {
    let use_keys = knn_args.key_uid || knn_args.key_bytes;

    if let Some(data) = knn_args.data {
        // Here we have CLI data - do this separately as we are just dispatching this as a batch
        let find_data = if use_keys {
            let keys = KeySet::parse_with_type(&data, knn_args.key_bytes)?;
            FindData::Keys(keys)
        } else {
            // TODO: This needs to be fixed with better batching and reporting
            let features = data
                .lines()
                .filter_map(|l| l.parse::<Feature>().ok())
                .collect();
            FindData::Features(features)
        };

        let knn = KnnReq {
            k: knn_args.k,
            r: knn_args.r,
            data: find_data,
        };

        handler.send(Request::Knn(knn))?;
    } else {
        // Here we have IO
        let input = Input::try_new(knn_args.file)?;

        if use_keys {
            // TODO: We are now assuming that we have lines of comma-separated keys
            // Is this the best assumption, should this be batched better?
            // TODO: Is this enumerate version good enough for error reporting? Should this differ by io vs. cli?
            // TODO: Maybe do something akin to the into_feature_iter for keys for this also
            let key_lines = input.lines().enumerate().filter_map(|(n, l)| {
                match l
                    .map_err(Into::<anyhow::Error>::into)
                    .and_then(|s| KeySet::parse_with_type(&s, knn_args.key_bytes))
                {
                    Ok(ks) => Some(ks),
                    Err(err) => {
                        eprintln!("Could not read line {}: {}", n, err);
                        None
                    }
                }
            });

            for ks in key_lines {
                let knn_req = KnnReq {
                    k: knn_args.k,
                    r: knn_args.r,
                    data: FindData::Keys(ks),
                };
                handler.send(Request::Knn(knn_req))?;
            }
        } else {
            // TODO: Here we are just doing one feature per request... batch this
            for feature_result in input.into_feature_iter() {
                match feature_result {
                    Ok((_, f)) => {
                        let knn_req = KnnReq {
                            k: knn_args.k,
                            r: knn_args.r,
                            data: FindData::Features(vec![f]),
                        };
                        handler.send(Request::Knn(knn_req))?;
                    }
                    // TODO: Harmonize interim error reporting, and decide if this is the best way
                    Err(err) => eprintln!("{}", err),
                }
            }
        }
    }

    Ok(())
}
