use std::{fs, path::PathBuf};

use geo_munge::error::Error;
use serde::Deserialize;

use crate::write::{write_cmp, write_data};

#[derive(Debug, Deserialize)]
pub struct RunSet {
    runs: Vec<SerializedRun>,
    name: String,
}

// TODO: Impl iter() and into_iter for refs, can we take as AsRef in the try_from?
impl RunSet {}

#[derive(Debug, Deserialize)]
struct SerializedRun {
    pub shape: RunGeometry,
    pub count: usize,
    pub meta: usize,
    pub cmp: usize,
    pub desc: Option<String>,
}

pub struct Run {
    pub name: String,
    pub index: usize,
    pub shape: RunGeometry,
    pub count: usize,
    pub meta: usize,
    pub cmp: usize,
    pub desc: String,
}

impl Run {
    /// Build a set of data and comparison point files for the run. Note that this only takes the
    /// main build directory without an additions for the name - that is done in here.
    pub fn build(&self, build_dir: &PathBuf) -> Result<(PathBuf, PathBuf), Error> {
        let mut build_dir = build_dir.clone();
        build_dir.push(&self.name);

        // Make sure the folder exists - this is a little redundand, but this is not a performance
        // critical application
        fs::create_dir_all(&build_dir).map_err(|err| Error::FileIOError(err))?;

        // Write out both the data and the comparison points files
        Ok((
            write_data(&build_dir, &self)?,
            write_cmp(&build_dir, &self)?,
        ))
    }
}

impl TryFrom<&PathBuf> for RunSet {
    type Error = Error;

    fn try_from(run_path: &PathBuf) -> Result<Self, Self::Error> {
        // Check that the run exists so we can give a meaningful error message
        if !run_path
            .try_exists()
            .map_err(|err| Error::FileIOError(err))?
        {
            eprintln!("The requested run is not loaded. Please load and try again.");
            return Err(Error::CannotReadFile(run_path.clone()));
        }
        let run_file = fs::File::open(&run_path).map_err(|err| Error::FileIOError(err))?;

        let name = run_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .trim_end_matches(".json")
            .to_string();

        let runs = serde_json::from_reader::<_, Vec<SerializedRun>>(&run_file)
            .map_err(|err| Error::FailedToDeserialize(run_path.clone(), err))?;

        Ok(RunSet { runs, name })
    }
}

impl IntoIterator for RunSet {
    type Item = Run;
    type IntoIter = RunIter;

    fn into_iter(self) -> Self::IntoIter {
        RunIter(Box::new(self.runs.into_iter().enumerate().map(
            move |(i, sr)| Run {
                name: self.name.clone(),
                index: i,
                shape: sr.shape,
                count: sr.count,
                meta: sr.meta,
                cmp: sr.cmp,
                desc: sr.desc.unwrap_or_default(),
            },
        )))
    }
}

/// Wrapper type for an iterator through a [`RunSet`].
/// Requires a wrapper rather than a type alias to avoid leaking the private run type.
pub struct RunIter(Box<dyn Iterator<Item = Run>>);
//pub struct RunIter(Map<Enumerate<IntoIter<SerializedRun>>, fn((usize, SerializedRun)) -> Run>);

/// Simple delegation for the [`RunIter`]'s iterator.
impl Iterator for RunIter {
    type Item = Run;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

/// Allowable Geometries in a `[Run]`.
#[derive(Debug, Deserialize)]
pub enum RunGeometry {
    #[serde(alias = "point", alias = "POINT")]
    Point,

    #[serde(alias = "linestring", alias = "LINESTRING")]
    LineString,
}

impl ToString for RunGeometry {
    fn to_string(&self) -> String {
        let str = match self {
            Self::Point => "Point",
            Self::LineString => "LineString",
        };

        str.to_string()
    }
}
