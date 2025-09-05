mod debug;
mod distance;
#[cfg(test)]
pub mod harness;
mod indexes;
mod math;
mod vector;

pub use distance::Distance;
pub use indexes::basic_quadtree::BasicQuadTree;
pub use indexes::*;
pub use math::earth_bbox;

/// Mean radius of Earth in meters.
///
/// This is the value recommended by the IUGG:
/// Moritz, H. (2000). Geodetic Reference System 1980. Journal of Geodesy, 74(1), 128ΓÇô133. doi:10.1007/s001900050278
/// "Derived Geometric Constants: mean radius" (p133)
/// https://link.springer.com/article/10.1007%2Fs001900050278
/// https://sci-hub.se/https://doi.org/10.1007/s001900050278
/// https://en.wikipedia.org/wiki/Earth_radius#Mean_radius
/// Extracted as-is from geo-rust source
pub const EARTH_RADIUS_METERS: f64 = 6371008.8;
