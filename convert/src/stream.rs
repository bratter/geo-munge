//! Input and output stream definitions.
//!
//! Monomorphized stream definitions to abstract across input and output streams.

use std::{
    fs::File,
    io::{stdin, stdout, BufRead, BufReader, Read, Stdin, Stdout, Write},
    path::PathBuf,
    str::FromStr,
};

#[cfg(test)]
use std::io::Cursor;
#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub enum StreamKind {
    StdIo,
    File(PathBuf),
    #[cfg(test)]
    String(String),

    /// Use an Arc<Mutex> to easily enable testing without using lifetimes (as non of the non-testing options need
    /// references). Cannot use Rc<RefCell> as this doesn't meeting parsing trait bounds.
    #[cfg(test)]
    OutputString(Arc<Mutex<String>>),
}

impl FromStr for StreamKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "-" {
            Ok(Self::StdIo)
        } else {
            Ok(Self::File(PathBuf::from(s)))
        }
    }
}

pub enum InputStream {
    Stdin(BufReader<Stdin>),
    File(BufReader<File>),
    #[cfg(test)]
    String(BufReader<Cursor<String>>),
}

impl InputStream {
    pub fn new(stream: StreamKind) -> Self {
        match stream {
            StreamKind::StdIo => InputStream::Stdin(BufReader::new(stdin())),
            StreamKind::File(path) => {
                // TODO: Make this fallible?
                let file = File::open(path).expect("Failed to open file");
                InputStream::File(BufReader::new(file))
            }
            #[cfg(test)]
            StreamKind::String(s) => InputStream::String(BufReader::new(Cursor::new(s))),
            #[cfg(test)]
            StreamKind::OutputString(_) => unreachable!("Should not be used"),
        }
    }
}

impl From<StreamKind> for InputStream {
    fn from(stream: StreamKind) -> Self {
        InputStream::new(stream)
    }
}

impl Read for InputStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            InputStream::Stdin(r) => r.read(buf),
            InputStream::File(r) => r.read(buf),
            #[cfg(test)]
            InputStream::String(r) => r.read(buf),
        }
    }
}

impl BufRead for InputStream {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        match self {
            InputStream::Stdin(r) => r.fill_buf(),
            InputStream::File(r) => r.fill_buf(),
            #[cfg(test)]
            InputStream::String(r) => r.fill_buf(),
        }
    }

    fn consume(&mut self, amt: usize) {
        match self {
            InputStream::Stdin(r) => r.consume(amt),
            InputStream::File(r) => r.consume(amt),
            #[cfg(test)]
            InputStream::String(r) => r.consume(amt),
        }
    }
}

pub enum OutputStream {
    Stdout(Stdout),
    File(File),
    #[cfg(test)]
    String(Arc<Mutex<String>>),
}

impl OutputStream {
    pub fn new(stream: StreamKind) -> Self {
        match stream {
            StreamKind::StdIo => OutputStream::Stdout(stdout()),
            StreamKind::File(path) => {
                // TODO: Make this fallible?
                let file = File::open(path).expect("Failed to open file");
                OutputStream::File(file)
            }
            #[cfg(test)]
            StreamKind::String(_) => unimplemented!("Should not be used"),
            #[cfg(test)]
            StreamKind::OutputString(s) => OutputStream::String(s),
        }
    }
}

impl From<StreamKind> for OutputStream {
    fn from(stream: StreamKind) -> Self {
        OutputStream::new(stream)
    }
}

impl Write for OutputStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            OutputStream::Stdout(w) => w.write(buf),
            OutputStream::File(w) => w.write(buf),
            #[cfg(test)]
            OutputStream::String(s) => {
                let str = std::str::from_utf8(buf).expect("Valid utf8");
                s.lock().unwrap().push_str(str);
                Ok(buf.len())
            }
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            OutputStream::Stdout(w) => w.flush(),
            OutputStream::File(w) => w.flush(),
            // No-op
            #[cfg(test)]
            OutputStream::String(_) => Ok(()),
        }
    }
}
