use std::{fs, path::PathBuf};

use geo_munge::error::Error;

use crate::run::RunSet;

/// Load the config at input_path into the runs directory.
pub fn load(input_path: &PathBuf, run_path: &PathBuf) -> Result<(), Error> {
    // Get the deserialized run set and attempt to copy if Ok, print error otherwise
    match RunSet::try_from(input_path) {
        Err(err) => println!("{err}"),
        Ok(_) => {
            // Fail if the file exists or can't resolve file name
            let fname = input_path.file_name().ok_or_else(|| {
                println!("Load failed: Input filename cannot be resolved");
                Error::CannotReadFile(input_path.clone())
            })?;

            let mut dest = run_path.clone();
            dest.push(fname);
            let exists = dest.try_exists().map_err(|err| {
                println!("Load failed: destination error {}", &err);
                Error::FileIOError(err)
            })?;

            if exists {
                println!(
                    "Load failed: Run file \"{}\" already exists at the destination",
                    &fname.to_string_lossy()
                );
                return Err(Error::CannotReadFile(input_path.clone()));
            } else {
                fs::copy(input_path, dest).map_err(|err| {
                    println!("Load failed: Copy error");
                    Error::FileIOError(err)
                })?;
            }
        }
    }

    Ok(())
}
