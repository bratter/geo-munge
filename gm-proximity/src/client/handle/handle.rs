use std::io::{Read, Write};

use anyhow::{bail, Result};

use crate::{args::ClientCommand, message::prelude::*};

use super::{knn, load, reset};

/// Client command handler.
pub struct CommandHandler<'a, R: Read, W: Write> {
    reader: &'a mut R,
    writer: &'a mut W,
}

impl<'a, R: Read, W: Write> CommandHandler<'a, R, W> {
    pub fn new(reader: &'a mut R, writer: &'a mut W) -> Self {
        Self { reader, writer }
    }

    // TODO: The handle function currently just sends a single request then blocks while awaiting a response.
    // Likely will want non-blocking on some calls so maybe this should just return then can await both client calls and
    // responses in a loop. May want to add a message id for fingerprinting / associating requests and responses.
    pub fn handle(&mut self, req: ClientCommand) -> Result<()> {
        match req {
            ClientCommand::Stats => {
                self.send(Request::Stats)?;
                self.block_on_response()?;
                Ok(())
            }
            ClientCommand::Reset(r) => reset(self, r),
            ClientCommand::Load { file } => load(self, file),
            ClientCommand::Knn(knn_args) => knn(self, knn_args),
        }?;

        Ok(())
    }

    /// Block awaiting a response, printing the result.
    pub fn block_on_response(&mut self) -> Result<()> {
        if let Some(res) = Response::read(self.reader)? {
            self.print_response(res);
            Ok(())
        } else {
            bail!("Server closed the connection when something was expected");
        }
    }

    pub fn send(&mut self, req: Request) -> Result<()> {
        req.write(self.writer)
    }

    // TODO: Upgrade response handling - ideally if the whole architecture was more message passing, this could be more
    // powerful than just echoing
    // TODO: As a minimum, add a context parameter
    // TODO: Have to call this inside the block_on_response due to liftimes (response lifetime is bound by self), maybe
    // consider cloning or other solutions if want to separate (which probably should)
    fn print_response(&mut self, res: Response) {
        match res {
            Response::Success(Some(msg)) => println!("{}", msg),
            Response::Success(None) => println!("success"),
            Response::Stats(n) => println!("The qt has {} items", n),
            Response::InsertResult { success, fail } => {
                println!("Inserted {}, failed {}", success, fail)
            }
            Response::KnnData(results) => println!("Knn: {:?}", results),
            Response::Data => println!("data"),
            Response::Error(msg) => println!("There was an error: {}", msg),
        }
    }
}
