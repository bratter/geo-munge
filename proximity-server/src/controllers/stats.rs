use network::connection::Traffic;
use protocol::prelude::*;

use super::Context;

// TODO: An actual implementation
pub fn stats(context: Context, traffic: &Traffic) {
    let store = context.store.load();

    let stats = Stats {
        key_mode: store.key_mode(),
        bbox: store.bbox().into(),
        len: store.len(),
        bytes_sent: traffic.sent(),
        bytes_recv: traffic.recv(),
    };

    context.send(Response::Stats(stats))
}
