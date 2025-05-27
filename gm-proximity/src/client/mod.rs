//! Client implementation for GM-Proximity.

mod handle;
mod input_io;
mod run;

pub use handle::{CommandHandler, Tracker};
pub use run::run;
pub use run::Config;
