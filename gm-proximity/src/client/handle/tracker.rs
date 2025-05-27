use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use anyhow::{anyhow, Error, Result};

use crate::message::prelude::*;

use super::ResponseHandler;

/// Newtype for a reuqest/response tracker.
///
/// Wraps an `Arc<Mutex<T>>` of the underling data structure that stores request/response pairings.
///
/// TODO: Need mechanism for passing more data through request to response using the tracker
/// TODO: Could consider a sparse ring buffer for this
/// TODO: Likely don't even need is oneshot any more as the handler will have it covered
#[derive(Default, Clone)]
pub struct Tracker(Arc<Mutex<BTreeMap<u32, TrackerData>>>);

impl Tracker {
    // TODO: See notes in ResponseHandler about the clone
    fn get(&self, id: u32) -> Result<TrackerData> {
        self.lock()
            .get(&id)
            .map(TrackerData::clone)
            .ok_or_else(|| Self::non_existent(id))
    }

    pub fn track(&self, id: u32, req: &Request, handler: ResponseHandler) {
        self.lock().insert(
            id,
            TrackerData {
                is_oneshot: req.is_oneshot(),
                start: Instant::now(),
                handler,
            },
        );
    }

    pub fn retire(&self, id: u32) -> Result<Duration> {
        self.lock()
            .remove(&id)
            .map(|td| td.start.elapsed())
            .ok_or_else(|| Self::non_existent(id))
    }

    pub fn is_oneshot(&self, id: u32) -> Result<bool> {
        self.lock()
            .get(&id)
            .map(|td| td.is_oneshot)
            .ok_or_else(|| Self::non_existent(id))
    }

    pub fn outstanding(&self) -> usize {
        self.lock().len()
    }

    fn lock(&self) -> MutexGuard<BTreeMap<u32, TrackerData>> {
        self.0.lock().expect("Lock poisoned")
    }

    fn non_existent(id: u32) -> Error {
        anyhow!("Non-existent id {}", id)
    }
}

// TODO: This is highly temporary - where should the handle call wind up?
// TODO: Upgrade response handling to actually route responses appropriately depending on the CLI options
// Might need to keep the request around if we need to know the context, or at least track more in the tracker
// Don't want to keep request around due to data, so need something in the tracker
// TODO: Think we'll have to pull the oneshot and retirement out of recv and move them in here. Also need to work
// out how to associate a Done response with a specific request type. Can we store a function pointer or a simple
// handler enum in the tracker? The handler enum can also store whatever data is required.
// TODO: Likely no errors from here, should just log if we can't find.
impl Tracker {
    pub fn handle(&mut self, id: u32, res: Response) -> Result<()> {
        let td = self.get(id)?;

        let is_oneshot = self.is_oneshot(id)?;
        let duration = if is_oneshot || matches!(res, Response::Done(_)) {
            Some(self.retire(id)?)
        } else {
            None
        };

        td.handler.handle(id, duration, &res);

        Ok(())
    }
}

// TODO: See notes in ResponseHandler about clone
#[derive(Clone)]
struct TrackerData {
    is_oneshot: bool,
    start: Instant,
    handler: ResponseHandler,
}
