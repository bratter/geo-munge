use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use crossbeam::channel::{Receiver, Sender};

use crate::{args::ClientCommand, message::prelude::*};

use super::{knn, load, reset};

/// Client command handler.
pub struct CommandHandler {
    request_tx: Sender<(u32, Request)>,
    response_rx: Receiver<(u32, Response)>,
    next_req_id: u32,
    /// Used to monitor requests and whether they have been retired or not.
    /// TODO: Could consider a sparse ring buffer for this
    tracker: BTreeMap<u32, (bool, Instant)>,
}

impl CommandHandler {
    pub fn new(request_tx: Sender<(u32, Request)>, response_rx: Receiver<(u32, Response)>) -> Self {
        Self {
            request_tx,
            response_rx,
            next_req_id: 0,
            tracker: BTreeMap::new(),
        }
    }

    pub fn handle(&mut self, req: ClientCommand) -> Result<()> {
        match req {
            ClientCommand::Stats => {
                self.send(Request::Stats)?;
                Ok(())
            }
            ClientCommand::Reset(r) => reset(self, r),
            ClientCommand::Load { file } => load(self, file),
            ClientCommand::Knn(knn_args) => knn(self, knn_args),
        }
    }

    /// Send a request for dispatch.
    ///
    /// Will block until the request channel has capacity.
    pub fn send(&mut self, req: Request) -> Result<()> {
        // Track the request before sending so we know when we have received responses
        let _ = self
            .tracker
            .insert(self.next_req_id, (req.is_oneshot(), Instant::now()));
        self.request_tx.send((self.next_req_id, req))?;

        // Increment the req_id after sending a request so we can keep track of which responses correspond to which
        // requesets
        self.next_req_id += 1;
        Ok(())
    }

    pub fn recv(&mut self) -> Result<(u32, Option<Duration>, Response)> {
        let res = self.response_rx.recv()?;
        let (is_oneshot, start) = *self
            .tracker
            .get(&res.0)
            .ok_or(anyhow!("Cannot find request record"))?;

        let mut duration = None;
        if is_oneshot || matches!(res.1, Response::Done(_)) {
            duration = Some(start.elapsed());
            self.tracker.remove(&res.0).expect("Already fetched");
        }

        Ok((res.0, duration, res.1))
    }

    pub fn outstanding_reqs(&self) -> usize {
        self.tracker.len()
    }

    // TODO: Upgrade response handling to actually route responses appropriately depending on the CLI options
    // Might need to keep the request around if we need to know the context
    pub fn print_response(&self, (req_id, duration, res): &(u32, Option<Duration>, Response)) {
        print!("[req {}", req_id);
        if let Some(duration) = duration {
            print!("; {}ms", duration.as_millis());
        }
        print!("] ");

        match res {
            Response::Success(Some(msg)) => println!("{}", msg),
            Response::Success(None) => println!("success"),
            Response::Done(n) => println!("done with {} responses", n),
            Response::Stats(n) => println!(
                "QT size={}; bytes sent={}; bytes recv={}",
                n.qt_size, n.bytes_sent, n.bytes_recv
            ),
            Response::InsertResult { success, fail } => {
                println!("Inserted {}, failed {}", success, fail)
            }
            Response::KnnData(results) => println!("Knn: {:?}", results),
            Response::Data => println!("data"),
            Response::Error(msg) => println!("There was an error: {}", msg),
        }
    }
}
