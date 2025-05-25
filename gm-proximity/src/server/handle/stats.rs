use crate::{connection::Traffic, message::prelude::*};

use super::Context;

// TODO: An actual implementation
pub fn stats(handler: Context, traffic: &Traffic) {
    let stats = Stats {
        qt_size: handler.read_qt().size(),
        bytes_sent: traffic.sent(),
        bytes_recv: traffic.recv(),
    };

    handler.send(Response::Stats(stats))
}
