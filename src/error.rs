use std::fmt;

/// Custom error enum for emitting on failure.
/// 
/// Includes custom display, debug traits to produce human-readable error messages.
#[non_exhaustive]
pub enum FiberError<'a> {
    IO(&'a str),
    Arg(&'a str),
    Parse(usize, &'a str),
}

impl std::error::Error for FiberError<'_> {}

impl fmt::Display for FiberError<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IO(e) => write!(f, "IO Error: {}", e),
            Self::Arg(e) => write!(f, "Argument Error: {}", e),
            Self::Parse(i, e) => write!(f, "Parse Error at line {}: {}", i, e),
        }
    }
}

// Custom debug implementation that delegates to Display
// This is then written on termination by the default Termination
// implementation
impl fmt::Debug for FiberError<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
