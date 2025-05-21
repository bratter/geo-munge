use crate::message::prelude::*;

use super::Context;

// TODO: An actual implementation
pub fn stats(handler: Context) {
    handler.send(Response::Stats(handler.read_qt().size()))
}
