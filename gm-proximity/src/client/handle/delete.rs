use crate::{args::DeleteArgs, message::prelude::*};

use anyhow::Result;

use super::CommandHandler;

/// Delete command handler.
///
/// Removes items from the server based on their primary or custom key.
pub fn delete(handler: &mut CommandHandler, del_args: DeleteArgs) -> Result<()> {
    let keys = KeySet::parse_with_type(&del_args.keys, del_args.key_bytes)?;

    handler.send(Request::Delete(keys))?;

    Ok(())
}
