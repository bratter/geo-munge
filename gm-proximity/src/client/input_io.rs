use std::{
    fs::File,
    io::{BufRead, BufReader, Lines, Read, Stdin},
    num::ParseIntError,
    path::Path,
};

use anyhow::{anyhow, bail, Result};

use crate::message::prelude::*;

/// An abstraction layer over file or stdin input streams, useful for abstracting over input types in the client CLI.
pub enum Input {
    File(BufReader<File>),
    Stdin(BufReader<Stdin>),
}

impl Input {
    /// Make a new input abstraction.
    ///
    /// When [`None`] will attempt to use stdio, erroring if it is a tty rather than a pipe. Otherwise will attempt to
    /// open the provided path.
    pub fn try_new(path: Option<impl AsRef<Path>>) -> Result<Self> {
        if let Some(path) = path {
            Ok(Self::try_from(path.as_ref())?)
        } else {
            Self::stdin()
        }
    }

    /// Directly attempt to make an input abstraction from stdin. Will fail if stdin is a tty.
    pub fn stdin() -> Result<Self> {
        if atty::isnt(atty::Stream::Stdin) {
            Ok(Self::Stdin(BufReader::new(std::io::stdin())))
        } else {
            bail!("Attempted to pipe from stdin, but it is not a pipe.")
        }
    }

    /// Convert the Input into an iterator of line-oriented bytes.
    pub fn into_data_stream_iter(self) -> DataStreamIterator {
        DataStreamIterator {
            inner: self.lines(),
        }
    }

    /// Convert the Input into an iterator of usize primary keys.
    pub fn into_key_iter(self) -> KeyIterator {
        KeyIterator {
            inner: self.lines(),
        }
    }
}

impl TryFrom<&Path> for Input {
    type Error = std::io::Error;

    fn try_from(value: &Path) -> std::result::Result<Self, Self::Error> {
        let reader = BufReader::new(File::open(value)?);
        Ok(Input::File(reader))
    }
}

impl Read for Input {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Input::File(file) => file.read(buf),
            Input::Stdin(stdin) => stdin.read(buf),
        }
    }
}

impl BufRead for Input {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        match self {
            Input::File(file) => file.fill_buf(),
            Input::Stdin(stdin) => stdin.fill_buf(),
        }
    }

    fn consume(&mut self, amt: usize) {
        match self {
            Input::File(file) => file.consume(amt),
            Input::Stdin(stdin) => stdin.consume(amt),
        }
    }
}

pub struct DataStreamIterator {
    inner: Lines<Input>,
}

impl Iterator for DataStreamIterator {
    type Item = Result<DataStream>;

    fn next(&mut self) -> Option<Self::Item> {
        let line_result = self.inner.next()?;

        match line_result {
            Ok(mut line) => {
                // TODO: Avoid recursion?
                if line.len() == 0 {
                    self.next()
                } else {
                    line.push('\n');
                    Some(Ok(DataStream::from(line.into_bytes())))
                }
            }
            Err(err) => Some(Err(anyhow!(err))),
        }
    }
}

pub struct KeyIterator {
    inner: Lines<Input>,
}

impl Iterator for KeyIterator {
    type Item = Result<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        let line_result = self.inner.next()?;

        match line_result {
            Ok(line) => {
                if line.len() == 0 {
                    self.next()
                } else {
                    Some(line.parse().map_err(|err: ParseIntError| anyhow!(err)))
                }
            }
            Err(err) => Some(Err(anyhow!(err))),
        }
    }
}
