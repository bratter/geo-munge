use anyhow::Result;
use protocol::prelude::*;

use crate::{args::KnnArgs, input_io::Input, message::dispatch_counted_batches};

use super::{
    print_and_filter_err, CommandHandler, Res, MAX_BATCH_BYTES, MAX_FEATURE_COUNT,
    MAX_ID_BATCH_SIZE,
};

/// Knn command handler.
pub fn knn(handler: &CommandHandler, res: &Res, knn_args: KnnArgs) -> Result<()> {
    let out_opts = knn_args.output_options();

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
            content_mode: out_opts.content_mode,
            start_index: 0,
            data: find_data,
        };

        handler.send_with_output(Request::Knn(knn_req), &res, out_opts)?;
    } else {
        // Handle IO data - per-line processing with individual error reporting
        handle_io_data(handler, &res, &knn_args)?;
    }

    Ok(())
}

fn handle_cli_data(data: String, key_uid: bool, key_bytes: bool) -> Result<FindData> {
    let use_keys = key_uid || key_bytes;

    if use_keys {
        let keys = KeySet::parse_with_type(&data, key_bytes);
        Ok(FindData::Keys(keys?))
    } else {
        let features: Result<Vec<_>, _> = data.lines().map(|l| l.parse::<JsonFeature>()).collect();
        Ok(FindData::Features(features?))
    }
}

fn handle_io_data(handler: &CommandHandler, res: &Res, knn_args: &KnnArgs) -> Result<()> {
    let input = match Input::try_new(knn_args.input.as_ref()) {
        Ok(input) => input,
        Err(err) => {
            eprintln!("Could not read input: {}", err);
            return Ok(());
        }
    };

    // Track the absolute index of the incoming request item
    let mut cur_index = 0;
    match (knn_args.key_uid, knn_args.key_bytes) {
        (true, true) => unreachable!("key_uid and key_bytes are mutually exclusive"),
        (true, false) => {
            // Process UIDs using batching
            let _ = dispatch_counted_batches(
                input.into_uid_iter().filter_map(print_and_filter_err),
                MAX_ID_BATCH_SIZE,
                |batch| {
                    let batch = FindData::Keys(KeySet::Uid(batch));
                    let batch_len = batch.len();
                    let res = send_knn_req(handler, res, knn_args, cur_index, batch);
                    cur_index += batch_len;
                    res
                },
            )?;
        }
        (false, true) => {
            // Process custom keys using batching
            let _ = dispatch_counted_batches(
                input
                    .into_custom_key_iter()
                    .filter_map(print_and_filter_err),
                MAX_ID_BATCH_SIZE,
                |batch| {
                    let batch = FindData::Keys(KeySet::Custom(batch));
                    let batch_len = batch.len();
                    let res = send_knn_req(handler, res, knn_args, cur_index, batch);
                    cur_index += batch_len;
                    res
                },
            )?;
        }
        (false, false) => {
            // Process features using batching
            // Note the description of the batching logic in the constant's docs and the load handler
            let mut feature_buffer = Vec::with_capacity(MAX_FEATURE_COUNT);
            let mut batch_bytes = 0;

            for (text_bytes, feature) in input.into_feature_iter().filter_map(print_and_filter_err)
            {
                batch_bytes += text_bytes;
                // is_empty condition required to ensure that we don't send empty buffers
                // Because we unconditionally push we won't skip individual items, which is what we want
                if batch_bytes >= MAX_BATCH_BYTES && !feature_buffer.is_empty() {
                    let batch = std::mem::replace(
                        &mut feature_buffer,
                        Vec::with_capacity(MAX_FEATURE_COUNT),
                    );
                    let features = FindData::Features(batch);
                    let feature_len = features.len();
                    send_knn_req(handler, res, knn_args, cur_index, features)?;
                    batch_bytes = 0;
                    cur_index += feature_len;
                }
                feature_buffer.push(feature);
            }

            // Final flush
            if feature_buffer.len() > 0 {
                let features = FindData::Features(feature_buffer);
                send_knn_req(handler, res, knn_args, cur_index, features)?;
            }
        }
    }

    Ok(())
}

fn send_knn_req(
    handler: &CommandHandler,
    res: &Res,
    knn_args: &KnnArgs,
    start_index: usize,
    data: FindData,
) -> Result<()> {
    let out_opts = knn_args.output_options();
    let knn_req = KnnReq {
        k: knn_args.k,
        r: knn_args.r,
        content_mode: out_opts.content_mode,
        start_index,
        data,
    };

    handler.send_with_output(Request::Knn(knn_req), res, out_opts)
}
