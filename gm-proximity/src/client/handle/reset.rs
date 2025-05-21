use anyhow::{bail, Result};
use dialoguer::Confirm;

use crate::{args::ResetArgs, message::prelude::*};

use super::CommandHandler;

/// Reset command handler
/// TODO: Add keytype settings
pub fn reset(handler: &mut CommandHandler, reset: ResetArgs) -> Result<()> {
    // Short circuit will avoid confirm if force is true
    if reset.force
        || Confirm::new()
            .with_prompt("Are you sure you want to reset the quadtree?")
            .interact_opt()?
            .unwrap_or(false)
    {
        handler.send(Request::Reset(Reset::new(None, reset.bbox)))?;
        Ok(())
    } else {
        bail!("Reset aborted")
    }
}
