//! Requests.

use std::{iter::FilterMap, slice::Split, str::FromStr};

use anyhow::{bail, Error, Result};
use bincode::{Decode, Encode};
use geo::{Point, Rect};
use geojson::Feature;

use super::MessageStream;

#[derive(Debug, Encode, Decode)]
#[non_exhaustive]
pub enum Request {
    /// Request basic quadtree statistics from the current server.
    ///
    /// Includes both config settings such as bounding box and primary key, and quadtree statistics such as siae and
    /// number of layers.
    Stats,

    /// Reset request.
    ///
    /// Truncate the quadtree. Options to reset the primary key and bounding box, otherwise will reset these to the
    /// defaults, although they can be changed prior to inserting.
    Reset(Reset),

    /// Define the primary key type to use.
    ///
    /// Can only be run on an empty Quadtree. Set the primary key mode to either auto-increment (the default) or to some
    /// metadata column with a type.
    ///
    /// TODO: Implement the ability to set the keytype
    KeyType(KeyType),

    /// Define the bounding box.
    ///
    /// Will default to the whole Earth bounding box of `[-180, -90, 180, 90]`. Anything outside the bounding box will
    /// be rejected. The bounding box can be grown after initialization, but it cannot be shrunk.
    ///
    /// The underlying quadtree implementation reserves the right to not set the bounding box exactly if it would be
    /// more efficient to choose a large bounding box. It will never be smaller.
    ///
    /// TODO: Implement the ability to set the bbox on a quadtree
    Bbox(Bbox),

    /// Insert request.
    ///
    /// Add one item to the quadtree. This is kept to a single item to avoid requiring the length of multiple items.
    Insert(Insert),

    /// Delete request.
    ///
    /// Delete an item using its primary key.
    Delete,

    /// Conduct a KNN search on the Quadtree.
    ///
    /// Can take a max count, radius or bounding box constraints, and metadata filters.
    Knn,

    /// Filter for all items inside a bounding box.
    ///
    /// Can take a set of metadata filters.
    ///
    /// TODO: Support grouping for close together items
    Window,
}

// Use default read/write impls.
impl MessageStream for Request {}

/// Reset request type.
///
/// Contains an optional key type and bounding box to provide settings in a single request.
#[derive(Debug, Default, Encode, Decode)]
pub struct Reset {
    pub keytype: Option<KeyType>,
    pub bbox: Option<Bbox>,
}

/// Key type setting request data.
///
/// Can be used in a [`Request::KeyType`], but more likely to be used in [`Request::Reset`].
#[derive(Debug, Default, Encode, Decode)]
pub enum KeyType {
    #[default]
    AutoIncrement,
    // TODO: Need to add the type here
    Meta(String),
}

/// Bounding box request data.
///
/// Can be used in a [`Request::Bbox`], but more likely to be used in [`Request::Reset`].
#[derive(Debug, Encode, Decode)]
pub struct Bbox {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
}

impl Default for Bbox {
    fn default() -> Self {
        Self {
            x1: -180.0,
            y1: -90.0,
            x2: 180.0,
            y2: 90.0,
        }
    }
}

impl From<Rect> for Bbox {
    fn from(value: Rect) -> Self {
        let min = value.min();
        let max = value.max();

        Bbox {
            x1: min.x,
            y1: min.y,
            x2: max.x,
            y2: max.y,
        }
    }
}

impl From<Bbox> for Rect {
    fn from(value: Bbox) -> Self {
        Rect::new(
            Point::new(value.x1, value.y1),
            Point::new(value.x2, value.y2),
        )
    }
}

impl FromStr for Bbox {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let entries: Result<Vec<_>, _> = s.split(',').map(|s| s.parse::<f64>()).collect();
        let entries = entries?;

        if entries.len() != 4 {
            bail!(
                "A bounding box requires 4 comma separated floats, {} provided",
                entries.len()
            );
        }

        // Now we know there are exactly 4 entries
        let x1 = entries[0];
        let y1 = entries[1];
        let x2 = entries[2];
        let y2 = entries[3];

        if x1 >= x2 || y1 >= y2 {
            bail!("A bounding box must be four floats x1,y1,x2,y2 with 1 being the top left and 2 being the bottom right");
        }

        Ok(Bbox { x1, y1, x2, y2 })
    }
}

/// Wrapper type for an insertion request.
///
/// This request makes no promises that the included data is well-formed JSON (so that clients don't have to validate
/// when the server really should anyway). Therefore items extracted from the insert may error on the server during
/// parsing. This SHOULD NOT error the whole insert, only the individual features.
///
/// If possible, clients SHOULD batch insertion requests to improve efficiency. Batches should be sized small enough to
/// avoid over-using memory, but can be larger than 1 to make inserts more efficient. The server MAY choose to
/// arbitrarily chunk large batches, but will not batch across requests.
#[derive(Debug, Default, Encode, Decode)]
pub struct Insert {
    data: Vec<u8>,
}

// TODO: This should take a lifetime and be generic over AsRef &[u8], unless this would be worse for Vecs (but think
// that the froms can just work with either
impl From<Vec<u8>> for Insert {
    fn from(data: Vec<u8>) -> Self {
        Insert { data }
    }
}

impl From<Insert> for Vec<u8> {
    fn from(value: Insert) -> Self {
        value.data
    }
}

impl<'a> IntoIterator for &'a Insert {
    type Item = Result<Feature>;

    type IntoIter = FilterMap<Split<'a, u8, fn(&u8) -> bool>, fn(&[u8]) -> Option<Result<Feature>>>;

    fn into_iter(self) -> Self::IntoIter {
        self.data
            .split(is_newline as fn(&u8) -> bool)
            .filter_map(filter_map as fn(&[u8]) -> Option<Result<Feature>>)
    }
}

fn is_newline(b: &u8) -> bool {
    *b == b'\n'
}

fn filter_map(line: &[u8]) -> Option<Result<Feature>> {
    let line = line.trim_ascii();
    if line.is_empty() {
        None
    } else {
        Some(parse_line(line))
    }
}

fn parse_line(line: &[u8]) -> Result<Feature> {
    Ok(std::str::from_utf8(line)?.parse::<Feature>()?)
}
