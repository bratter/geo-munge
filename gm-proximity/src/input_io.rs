use std::{
    fs::File,
    io::{BufRead, BufReader, Lines, Read, Stdin},
    path::Path,
};

use anyhow::{bail, Result};

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

    /// Convert the Input into an iterator of line-oriented [`Feature`] types.
    pub fn into_feature_iter(self) -> FeatureIterator {
        FeatureIterator::new(self)
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

pub struct FeatureIterator {
    inner: Lines<Input>,
    count: usize,
}

impl FeatureIterator {
    pub fn new(input: Input) -> Self {
        Self {
            inner: input.lines(),
            count: 0,
        }
    }
}

impl Iterator for FeatureIterator {
    type Item = Feature;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(line_result) = self.inner.next() {
            match line_result
                .map_err(Into::<anyhow::Error>::into)
                .and_then(|s| Ok(s.parse::<Feature>()?))
            {
                Ok(f) => {
                    self.count += 1;
                    return Some(f);
                }
                Err(err) => {
                    eprintln!("Could not read line {}: {}", self.count, err);
                    self.count += 1;
                    continue;
                }
            }
        }
        None
    }
}
