use std::{
    collections::HashMap,
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
/// NOTE: As this operates in a largely single-command at a time client, wrapping the whole tracker in an Arc<Mutex<_>>
/// should not cause lock contention, hence using a simple, low overhead approach. Additionally, we clone the whole
/// tracker data rather than wrapping it in an Arc. This decision could be revisited if the tracker data gets large or
/// changes any state when responses come in (e.g., counts responses to check for loss)
///
/// TODO: Need mechanism for passing more data through request to response using the tracker
/// TODO: Likely don't even need is oneshot any more as the handler will have it covered
#[derive(Default, Clone)]
pub struct Tracker(Arc<Mutex<HashMap<u32, TrackerData>>>);

impl Tracker {
    fn get(&self, id: u32) -> Result<TrackerData> {
        // Checking that the tracker data doesn't get too big to clone
        // If this gets thrown, consider converting to Arc
        debug_assert!(std::mem::size_of::<Self>() <= 64);

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

    fn lock(&self) -> MutexGuard<HashMap<u32, TrackerData>> {
        self.0.lock().expect("Lock poisoned")
    }

    fn non_existent(id: u32) -> Error {
        anyhow!("Non-existent id {}", id)
    }
}

// TODO: Upgrade response handling to actually route responses appropriately depending on the CLI options
// Might need to keep the request around if we need to know the context, or at least track more in the tracker
// Don't want to keep request around due to data, so need something in the tracker
// TODO: Think we'll have to pull the oneshot and retirement out of recv and move them in here. Also need to work
// out how to associate a Done response with a specific request type. Can we store a function pointer or a simple
// handler enum in the tracker? The handler enum can also store whatever data is required.
// TODO: Likely no errors from here, should just log if we can't find.
// FIX: At least ensure that this error gets logged
impl Tracker {
    pub fn handle(&mut self, id: u32, res: Response) -> Result<()> {
        // We return with an error if the
        let tracker_data = self.get(id)?;
        let is_oneshot = self.is_oneshot(id)?;

        let done = if is_oneshot || matches!(res, Response::Done(_)) {
            Some(self.retire(id)?)
        } else {
            None
        };

        tracker_data.handler.handle(id, done, &res);

        Ok(())
    }
}

#[derive(Clone)]
struct TrackerData {
    is_oneshot: bool,
    start: Instant,
    handler: ResponseHandler,
}
