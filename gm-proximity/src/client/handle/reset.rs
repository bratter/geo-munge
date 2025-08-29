use anyhow::{bail, Result};
use dialoguer::Confirm;

use crate::{args::ResetArgs, message::prelude::*};

use super::{CommandHandler, Res};

/// Reset command handler
pub fn reset(handler: &CommandHandler, res: &Res, reset: ResetArgs) -> Result<()> {
    // Short circuit will avoid confirm if force is true
    if reset.force
        || Confirm::new()
            .with_prompt("Are you sure you want to reset the geo store?")
            .interact_opt()?
            .unwrap_or(false)
    {
        // key int and key bytes are mutually exclusive, so can test in turn
        let key_mode = if let Some(ptr) = reset.key_int {
            KeyMode::CustomU32(ptr)
        } else if let Some(ptr) = reset.key_bytes {
            KeyMode::MetaPointer(ptr)
        } else {
            KeyMode::AutoIncrement
        };

        handler.send(Request::Reset(ResetReq::new(key_mode, reset.bbox)), res)?;
        Ok(())
    } else {
        bail!("Reset aborted")
    }
}
