use crate::{connection::Traffic, message::prelude::*};

use super::Context;

// TODO: An actual implementation
pub fn stats(context: Context, traffic: &Traffic) {
    let stats = Stats {
        key_mode: (&**context.key_gen.load()).into(),
        qt_size: context.store.load().size(),
        bytes_sent: traffic.sent(),
        bytes_recv: traffic.recv(),
    };

    context.send(Response::Stats(stats))
}
