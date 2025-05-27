use anyhow::Result;

use crate::{args, message::prelude::*};

use super::{CommandHandler, Input, ResponseHandler};

/// Knn command handler
/// TODO: Fix this - it is a bare minimum test version - which at least is reporting results, even if the results don't
/// numbers don't look right
/// TODO: See notes in the load handler - improvements here will be similiar
/// TODO: After those are done, then try to simplify - e.g. abstract the 4 iterators behind an impl
pub fn knn(handler: &mut CommandHandler, knn_args: args::KnnArgs) -> Result<()> {
    if let Some(data) = knn_args.data {
        // Here we have CLI data - do this separately as we are just dispathcing this as a batch
        let find_data = if knn_args.test_keys {
            let keys = data
                .split(',')
                .map(|d| d.parse())
                .enumerate()
                .inspect(|(n, key)| {
                    if let Err(err) = key {
                        eprintln!("Could not read line {}: {}", n, err);
                    }
                })
                .filter_map(|(_, key)| key.ok())
                .collect();
            FindData::Keys(keys)
        } else {
            FindData::Geom(DataStream::from(data.into_bytes()))
        };

        let knn = Knn {
            k: knn_args.k,
            r: knn_args.r,
            data: find_data,
        };

        handler.send(Request::Knn(knn), ResponseHandler::None)?;
    } else {
        // Here we have IO
        let input = Input::try_new(knn_args.file)?;

        if knn_args.test_keys {
            for (n, data_result) in input.into_key_iter().enumerate() {
                match data_result {
                    Ok(data) => {
                        let knn = Knn {
                            k: knn_args.k,
                            r: knn_args.r,
                            data: FindData::Keys(vec![data]),
                        };

                        handler.send(Request::Knn(knn), ResponseHandler::None)?;
                    }
                    Err(err) => eprintln!("Could not read line {}: {}", n, err),
                }
            }
        } else {
            for (n, data_result) in input.into_data_stream_iter().enumerate() {
                match data_result {
                    Ok(data) => {
                        let knn = Knn {
                            k: knn_args.k,
                            r: knn_args.r,
                            data: FindData::Geom(data),
                        };

                        handler.send(Request::Knn(knn), ResponseHandler::None)?;
                    }
                    Err(err) => eprintln!("Could not read line {}: {}", n, err),
                }
            }
        }
    }

    Ok(())
}
