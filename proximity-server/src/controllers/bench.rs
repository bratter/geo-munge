use std::time::Duration;

use protocol::prelude::*;

use super::Context;

pub fn bench(handler: Context, bench: BenchReq) {
    tracing::trace!("Bench req: {:?}", bench);

    let delay = bench.delay.map(Duration::from_millis);

    for _ in 0..bench.ratio {
        // Delay applies to each response, not request
        if let Some(t) = delay {
            std::thread::sleep(t);
        }

        let bench_res = BenchRes::with_len(bench.size as usize);
        handler.send(Response::Bench(bench_res));
    }

    handler.send(Response::Done(bench.ratio));
}
