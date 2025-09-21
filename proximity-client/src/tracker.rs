use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex, MutexGuard,
    },
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Error, Result};
use protocol::prelude::*;

use super::handler::{OutputOptions, ResponseHandler, ResponseMeta};

/// Newtype for a reuqest/response tracker.
///
/// Wraps an `Arc<Mutex<T>>` of the underling data structure that stores request/response pairings.
///
/// NOTE: As this operates in a largely single-command at a time client, wrapping the whole tracker in an Arc<Mutex<_>>
/// should not cause lock contention, hence using a simple, low overhead approach.
#[derive(Default, Clone)]
pub struct Tracker(Arc<Mutex<HashMap<u32, Arc<TrackerData>>>>);

impl Tracker {
    fn get(&self, id: u32) -> Result<Arc<TrackerData>> {
        self.lock()
            .get(&id)
            .map(Arc::clone)
            .ok_or_else(|| Self::non_existent(id))
    }

    pub fn track(
        &self,
        id: u32,
        req: &Request,
        handler: Arc<Mutex<ResponseHandler>>,
        output_options: OutputOptions,
    ) {
        self.lock().insert(
            id,
            Arc::new(TrackerData {
                is_oneshot: req.is_oneshot(),
                start: Instant::now(),
                handler,
                output_options,
                response_count: AtomicU32::new(0),
            }),
        );
    }

    pub fn retire(&self, id: u32) -> Result<Duration> {
        self.lock()
            .remove(&id)
            .map(|td| td.start.elapsed())
            .ok_or_else(|| Self::non_existent(id))
    }

    pub fn outstanding(&self) -> usize {
        self.lock().len()
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<u32, Arc<TrackerData>>> {
        self.0.lock().expect("Lock poisoned")
    }

    fn non_existent(id: u32) -> Error {
        anyhow!("Non-existent id {}", id)
    }
}

// TODO: Consider keeping the whole request around in the TrackerData if it seems that it contains other useful data
impl Tracker {
    /// Associate an incoming response with its stored tracker data.
    ///
    /// Will then call the appropriate response handler from the tracking data if it can be found. If the tracking data
    /// can't be found, this indicates a critical error in the system due to lost data, and the client should likely
    /// abort.
    pub fn handle(&mut self, req_id: u32, res: Response) -> Result<()> {
        // We return with an error if the id can't be found in the tracker - this should be treated as a critical
        // failure by the system.
        let tracker_data = self.get(req_id)?;

        // Actual response count doesn't include the current response, but neither does the expected count in Done
        let res_id = tracker_data.response_count.fetch_add(1, Ordering::Relaxed);

        let done = if tracker_data.is_oneshot || matches!(res, Response::Done(_)) {
            if let Response::Done(expected_response_count) = res {
                if res_id != expected_response_count {
                    bail!(
                        "Response count mismatch, expecting {}, recieved {}",
                        expected_response_count,
                        res_id
                    )
                }
            }

            tracing::debug!("Retiring request with id {}", req_id);
            Some(self.retire(req_id)?)
        } else {
            None
        };

        let meta = ResponseMeta {
            req_id,
            res_id,
            done,
            output_options: tracker_data.output_options,
        };

        tracker_data
            .handler
            .lock()
            .expect("Lock poisoned")
            .handle(meta, res)
    }
}

struct TrackerData {
    is_oneshot: bool,
    start: Instant,
    handler: Arc<Mutex<ResponseHandler>>,
    output_options: OutputOptions,
    response_count: AtomicU32,
}
