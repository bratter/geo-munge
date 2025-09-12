use anyhow::{bail, Result};
use dialoguer::Confirm;
use protocol::prelude::*;

use crate::args::ResetArgs;

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
            KeyMode::U32Pointer(ptr)
        } else if let Some(ptr) = reset.key_bytes {
            KeyMode::MetaPointer(ptr)
        } else if reset.key_json_id {
            KeyMode::GeoJsonId
        } else {
            KeyMode::AutoIncrement
        };

        handler.send(Request::Reset(ResetReq::new(key_mode, reset.bbox)), res)?;
        Ok(())
    } else {
        bail!("Reset aborted")
    }
}
