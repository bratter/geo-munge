//! Handler to manage dispatch logic between requests and their controllers.

use std::sync::Arc;

use arc_swap::ArcSwap;
use network::connection::{MsgToken, Traffic};
use protocol::{prelude::BasicResult, ContentMode, Request, Response};
use proximity_ipc::channel::ResponseSender;

use super::controllers;
use crate::geo::{Feature, GeoStore, KeyGenerator};

pub struct Handler {
    store: ArcSwap<GeoStore>,
    response_tx: ResponseSender,
    key_gen: ArcSwap<KeyGenerator>,
}

impl Handler {
    pub fn new(store: ArcSwap<GeoStore>, response_tx: ResponseSender) -> Self {
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
            Request::Stats => controllers::stats(context, traffic),
            Request::Reset(r) => controllers::reset(context, r),
            Request::Insert(i) => controllers::insert(context, i),
            Request::Get(get_req) => controllers::get(context, get_req),
            Request::Delete(key_set) => controllers::delete(context, key_set),
            Request::Knn(knn_data) => controllers::knn(context, knn_data),
            Request::Window(window_data) => controllers::window(context, window_data),
            Request::Bench(bench_data) => controllers::bench(context, bench_data),
            // TODO: Change black hole of unknown request types
            _ => {}
        }
    }
}

/// Cheap container for data structure and channel access.
pub struct Context<'a> {
    pub store: &'a ArcSwap<GeoStore>,
    pub key_gen: &'a ArcSwap<KeyGenerator>,
    response_tx: ResponseSender,
    msg_token: MsgToken,
}

impl<'a> Context<'a> {
    pub fn new(
        store: &'a ArcSwap<GeoStore>,
        key_gen: &'a ArcSwap<KeyGenerator>,
        response_tx: ResponseSender,
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
    let content = record.generate_content(content_mode);

    BasicResult {
        id: record.id,
        content,
    }
}

#[cfg(test)]
mod test {
    use crossbeam::channel::Receiver;
    use proximity_ipc::{channel::ServerChannels, codec::IpcResponse};

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
        ) -> (Receiver<(MsgToken, IpcResponse)>, Self) {
            let channels = ServerChannels::new(1024, 1024);
            let context = Self::new(&store, &key_gen, channels.response_tx, token);

            (channels.response_rx, context)
        }
    }
}
