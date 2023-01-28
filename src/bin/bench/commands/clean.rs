use std::{fs, path::PathBuf};

use geo_munge::error::Error;

/// Clear all the build assets from the builds folder.
pub fn clean(build_path: &PathBuf) -> Result<(), Error> {
    if build_path
        .try_exists()
        .map_err(|err| Error::FileIOError(err))?
    {
        fs::remove_dir_all(&build_path).map_err(|err| Error::FileIOError(err))
    } else {
        Ok(())
    }
}
