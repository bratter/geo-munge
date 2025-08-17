use anyhow::Result;

use crate::{args::WindowArgs, message::prelude::*};

use super::CommandHandler;

/// Window command handler
pub fn window(handler: &mut CommandHandler, window_args: WindowArgs) -> Result<()> {
    let bbox = window_args.bbox;
    let join = if window_args.intersects {
        JoinType::Intersects
    } else {
        JoinType::Contains
    };

    handler.send(Request::Window(WindowReq { bbox, join }))
}
