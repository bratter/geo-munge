// TODO: Improve error implementation
#[derive(Debug)]
pub struct FiberError;

impl std::fmt::Display for FiberError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "A fiberdist error occured.")
    }
}

impl std::error::Error for FiberError {}
