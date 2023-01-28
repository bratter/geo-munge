use std::fs;

use geo_munge::error::Error;

use crate::Paths;

pub fn remove(name: &String, paths: &Paths) -> Result<(), Error> {
    // Clear any build assets
    let mut build_dir = paths.build.clone();
    build_dir.push(name);
    if build_dir
        .try_exists()
        .map_err(|err| Error::FileIOError(err))?
    {
        fs::remove_dir_all(&build_dir).map_err(|err| Error::FileIOError(err))?;
    }

    // Remove the run definition
    let mut run_file = paths.run.clone();
    run_file.push(format!("{name}.json"));

    fs::remove_file(&run_file).map_err(|err| Error::FileIOError(err))
}
