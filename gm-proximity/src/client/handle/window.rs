use anyhow::Result;

use crate::{args::WindowArgs, message::prelude::*};

use super::CommandHandler;

/// Window command handler
pub fn window(handler: &CommandHandler, window_args: WindowArgs) -> Result<()> {
    let join = if window_args.intersects {
        JoinType::Intersects
    } else {
        JoinType::Contains
    };

    handler.send(Request::Window(WindowReq {
        bbox: window_args.bbox,
        join,
        content_mode: window_args.content,
    }))
}
