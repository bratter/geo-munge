use std::fs;

use geo_munge::error::Error;

use crate::{run::RunSet, Paths};

pub fn build(name: &String, paths: &Paths) -> Result<(), Error> {
    // Open the run file and load
    let mut run_path = paths.run.clone();
    run_path.push(format!("{name}.json"));
    let run_set: RunSet = RunSet::try_from(&run_path)?;

    // TODO: Consider cleanup if writing fails rather than just aborting
    // Clean any existing folder and create fresh
    let mut build_dir = paths.build.clone();
    build_dir.push(name);
    if build_dir
        .try_exists()
        .map_err(|err| Error::FileIOError(err))?
    {
        fs::remove_dir_all(&build_dir).map_err(|err| Error::FileIOError(err))?;
    }
    fs::create_dir_all(&build_dir).map_err(|err| Error::FileIOError(err))?;

    // Iterate over all the runs to build
    // // Passing the raw build path as build will add the name
    for run in run_set {
        run.build(&paths.build)?;
    }

    Ok(())
}
