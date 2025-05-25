use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crossbeam::channel::Sender;
use geolib::qt::Quadtree;

use crate::connection::{MsgToken, Traffic};
use crate::message::prelude::*;

use super::{insert, knn, reset, stats};

#[derive(Clone)]
pub struct Handler {
    qt: Arc<RwLock<Quadtree>>,
    response_tx: Sender<(MsgToken, Response)>,
}

impl Handler {
    pub fn new(qt: Arc<RwLock<Quadtree>>, response_tx: Sender<(MsgToken, Response)>) -> Self {
        Self { qt, response_tx }
    }

    /// Generate a [`Context`] to pass around with this request.
    pub fn context(&self, msg_token: MsgToken) -> Context {
        Context::new(Arc::clone(&self.qt), self.response_tx.clone(), msg_token)
    }

    /// Handle an incoming request.
    /// TODO: Add timing to wrap the requests?
    pub fn handle(&self, (msg_token, req): (MsgToken, Request), traffic: &Traffic) {
        let context = self.context(msg_token);

        // TODO: These handlers currently don't return a result. This does mean they miss failures in the channel, and that
        // all other errors are appropriate just to send to the client. Can revist this decision.
        match req {
            Request::Stats => stats(context, traffic),
            Request::Reset(r) => reset(context, r),
            Request::KeyType(_) => {
                context.send(Response::Error("KeyType resetting not implemented".into()))
            }
            Request::Bbox(_) => {
                context.send(Response::Error("BBox resetting not implemented".into()))
            }
            Request::Insert(i) => insert(context, i),
            Request::Delete => context.send(Response::Error("Delete not implemented".into())),
            Request::Knn(knn_data) => knn(context, knn_data),
            Request::Window => {
                context.send(Response::Error("Window queries not implemented".into()))
            }
        }
    }
}

/// Cheap container for data structure and channel access.
pub struct Context {
    qt: Arc<RwLock<Quadtree>>,
    response_tx: Sender<(MsgToken, Response)>,
    msg_token: MsgToken,
}

impl Context {
    pub fn new(
        qt: Arc<RwLock<Quadtree>>,
        response_tx: Sender<(MsgToken, Response)>,
        msg_token: MsgToken,
    ) -> Self {
        Self {
            qt,
            response_tx,
            msg_token,
        }
    }

    pub fn read_qt(&self) -> RwLockReadGuard<'_, Quadtree> {
        // NOTE: Ok to propagate panic with unwrap as the only error is for a poisoned RwLock
        self.qt.read().unwrap()
    }

    pub fn write_qt(&self) -> RwLockWriteGuard<'_, Quadtree> {
        // NOTE: Ok to propagate panic with unwrap as the only error is for a poisoned RwLock
        self.qt.write().unwrap()
    }

    pub fn send(&self, response: Response) {
        // TODO: See note above, deliberately ignoring errors for now
        let _ = self.response_tx.send((self.msg_token, response));
    }
}

#[cfg(test)]
mod test {
    use crossbeam::channel::{self, Receiver};

    use crate::server::run::build_qt;

    use super::*;

    impl Context {
        /// Implementation to make a dummy context for testing purposes only using a fresh quadtree and a also returning
        /// the rx end of the response channel.
        pub fn test_new(token: MsgToken) -> (Receiver<(MsgToken, Response)>, Self) {
            let (tx, rx) = channel::unbounded();
            let qt = Arc::new(RwLock::new(build_qt(Reset::default())));
            let handler = Self::new(qt, tx, token);

            (rx, handler)
        }
    }
}
