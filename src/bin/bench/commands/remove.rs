use std::{fs, path::PathBuf};

use geo_munge::error::Error;

use crate::Paths;

/// Remove one specific run set and its associated build data.
pub fn remove(name: &String, paths: &Paths) -> Result<(), Error> {
    remove_build(name, &paths.build)?;
    remove_run(name, &paths.run)
}

pub fn remove_build(name: &String, build_dir: &PathBuf) -> Result<(), Error> {
    let mut build_dir = build_dir.clone();
    build_dir.push(name);

    if build_dir
        .try_exists()
        .map_err(|err| Error::FileIOError(err))?
    {
        fs::remove_dir_all(&build_dir).map_err(|err| Error::FileIOError(err))
    } else {
        Ok(())
    }
}

pub fn remove_run(name: &String, run_dir: &PathBuf) -> Result<(), Error> {
    let mut run_file = run_dir.clone();
    run_file.push(format!("{name}.json"));

    fs::remove_file(&run_file).map_err(|err| Error::FileIOError(err))
}
