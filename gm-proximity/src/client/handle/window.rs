use anyhow::Result;

use crate::{args::WindowArgs, message::prelude::*};

use super::{CommandHandler, Res};

/// Window command handler
pub fn window(handler: &CommandHandler, res: &Res, window_args: WindowArgs) -> Result<()> {
    let join = if window_args.intersects {
        JoinType::Intersects
    } else {
        JoinType::Contains
    };

    let req = Request::Window(WindowReq {
        bbox: window_args.bbox,
        join,
        content_mode: window_args.content,
    });

    handler.send(req, &res)
}
