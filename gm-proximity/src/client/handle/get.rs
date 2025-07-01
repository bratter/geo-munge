use crate::{args::GetArgs, message::prelude::*};

use anyhow::Result;

use super::CommandHandler;

/// Get command handler.
///
/// Gets store entries using their primary or custom key. Sends all as a single batch.
pub fn get(handler: &mut CommandHandler, get_args: GetArgs) -> Result<()> {
    let keys = KeySet::parse_with_type(&get_args.keys, get_args.key_bytes)?;

    handler.send(Request::Get(GetReq {
        keys,
        meta_only: get_args.meta_only,
    }))?;

    Ok(())
}
