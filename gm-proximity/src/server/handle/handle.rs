use std::sync::Arc;

use arc_swap::ArcSwap;
use crossbeam::channel::Sender;

use crate::connection::{MsgToken, Traffic};
use crate::message::prelude::*;
use crate::server::geo_store::GeoStore;

use super::handlers;

pub struct Handler {
    store: ArcSwap<GeoStore>,
    response_tx: Sender<(MsgToken, Response)>,
    key_gen: ArcSwap<KeyGenerator>,
}

impl Handler {
    pub fn new(store: ArcSwap<GeoStore>, response_tx: Sender<(MsgToken, Response)>) -> Self {
        let key_gen = ArcSwap::from(Arc::new(KeyGenerator::default()));

        Self {
            store,
            response_tx,
            key_gen,
        }
    }

    /// Generate a [`Context`] to pass around with this request.
    pub fn context(&self, msg_token: MsgToken) -> Context {
        Context::new(
            &self.store,
            &self.key_gen,
            self.response_tx.clone(),
            msg_token,
        )
    }

    /// Handle an incoming request.
    /// TODO: Add timing to wrap the requests?
    pub fn handle(&self, (msg_token, req): (MsgToken, Request), traffic: &Traffic) {
        tracing::trace!("Handling {:?}", msg_token);

        let context = self.context(msg_token);

        // TODO: These handlers currently don't return a result. This does mean they miss failures in the channel, and that
        // all other errors are appropriate just to send to the client. Can revist this decision.
        match req {
            Request::Stats => handlers::stats(context, traffic),
            Request::Reset(r) => handlers::reset(context, r),
            Request::Insert(i) => handlers::insert(context, i),
            Request::Get(get_req) => handlers::get(context, get_req),
            Request::Delete(key_set) => handlers::delete(context, key_set),
            Request::Knn(knn_data) => handlers::knn(context, knn_data),
            Request::Window(window_data) => handlers::window(context, window_data),
            Request::Bench(bench_data) => handlers::bench(context, bench_data),
        }
    }
}

/// Cheap container for data structure and channel access.
pub struct Context<'a> {
    pub store: &'a ArcSwap<GeoStore>,
    pub key_gen: &'a ArcSwap<KeyGenerator>,
    response_tx: Sender<(MsgToken, Response)>,
    msg_token: MsgToken,
}

impl<'a> Context<'a> {
    pub fn new(
        store: &'a ArcSwap<GeoStore>,
        key_gen: &'a ArcSwap<KeyGenerator>,
        response_tx: Sender<(MsgToken, Response)>,
        msg_token: MsgToken,
    ) -> Self {
        Self {
            store,
            key_gen,
            response_tx,
            msg_token,
        }
    }

    pub fn send(&self, response: Response) {
        // TODO: See note above, deliberately ignoring errors for now
        let _ = self.response_tx.send((self.msg_token, response));
    }
}

#[inline]
pub fn feature_to_basic_result(content_mode: ContentMode, record: &Feature) -> BasicResult {
    let content = content_mode.with_feature(record);

    BasicResult {
        id: record.id,
        content,
    }
}

#[cfg(test)]
mod test {
    use crossbeam::channel::{self, Receiver};

    use super::*;

    impl<'a> Context<'a> {
        pub fn make_store() -> (ArcSwap<GeoStore>, ArcSwap<KeyGenerator>) {
            (
                ArcSwap::from(Arc::new(GeoStore::default())),
                ArcSwap::from(Arc::new(KeyGenerator::default())),
            )
        }

        /// Implementation to make a dummy context for testing purposes only using a fresh quadtree and a also returning
        /// the rx end of the response channel.
        pub fn test_new(
            (store, key_gen): &'a (ArcSwap<GeoStore>, ArcSwap<KeyGenerator>),
            token: MsgToken,
        ) -> (Receiver<(MsgToken, Response)>, Self) {
            let (tx, rx) = channel::unbounded();
            let context = Self::new(&store, &key_gen, tx, token);

            (rx, context)
        }
    }
}
