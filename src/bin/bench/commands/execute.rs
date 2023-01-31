use std::{
    path::PathBuf,
    process::{Command, Stdio},
    time::Instant,
};

use geo_munge::error::Error;

use crate::{run::RunSet, Paths};

use super::remove_build;

// TODO: Meta capability
// TODO: Different shape capability
// TODO: Pass depth and children options, and search options (k, not p)?
// TODO: Add verbose logging (e.g. progress on long runs)?
pub fn execute(name: &String, n: usize, bin: Option<PathBuf>, paths: &Paths) -> Result<(), Error> {
    // Open the run file and load
    let mut run_path = paths.run.clone();
    run_path.push(format!("{name}.json"));
    let run_set: RunSet = RunSet::try_from(&run_path)?;

    // Find the location of the proximity binary
    // First use the argument, erroring immediately if the arg doesn't work
    // Otherwise the current directory, then finally path
    // TODO: Improve check exists - is it an executable?
    let bin = match bin {
        Some(path) => check_exists(path),
        None => check_exists("./proximity").or_else(|| check_exists("proximity")),
    }
    .ok_or(Error::CannotFindCommand)?;

    // Set up a csv writer for stdout for the results
    let mut writer = csv::Writer::from_writer(std::io::stdout());
    writer
        .write_record([
            "name",
            "index",
            "iteration",
            "shape",
            "data_count",
            "cmp_count",
            "time",
            "desc",
        ])
        .map_err(|err| Error::CsvWriteError(err))?;

    for run in run_set {
        // TODO: If we build a into_iter for a reference, then this can be done in the iterator
        for i in 0..n {
            // Build the assets
            match run.build(&paths.build) {
                Err(err) => eprintln!(
                    "File creation for run index {} failed. Error recieved: {}",
                    run.index, err
                ),
                Ok((data_path, cmp_path)) => {
                    let start = Instant::now();

                    let cat_stdout = Command::new("cat")
                        .arg(cmp_path)
                        .stdout(Stdio::piped())
                        .spawn()
                        .ok()
                        .and_then(|proc| proc.stdout)
                        .and_then(|cat_stdout| {
                            Command::new(&bin)
                                .arg("-p")
                                .arg("-s")
                                .arg(data_path)
                                .stdin(Stdio::from(cat_stdout))
                                .stdout(Stdio::null())
                                .spawn()
                                .ok()
                        })
                        // Run the pipeline
                        .and_then(|proximity_proc| proximity_proc.wait_with_output().ok());

                    let elapsed = start.elapsed().as_millis();

                    // Write results if we succeeded, error msg if not
                    match cat_stdout {
                        Some(_) => {
                            if writer
                                .write_record([
                                    run.name.to_string(),
                                    run.index.to_string(),
                                    i.to_string(),
                                    run.shape.to_string(),
                                    run.count.to_string(),
                                    run.cmp.to_string(),
                                    elapsed.to_string(),
                                    run.desc.to_string(),
                                ])
                                .is_err()
                            {
                                eprintln!(
                                    "Run succeded but could not write output for index {}, iter {}",
                                    run.index, i
                                );
                            }
                        }
                        None => eprintln!("Unable to process index {}, iter {}", run.index, i),
                    }

                    // Clean anyway
                    if remove_build(&run.name, &paths.build).is_err() {
                        eprintln!("Failed to remove build for index {}, iter {}", run.index, i);
                    }
                }
            }
        }
    }

    Ok(())
}

fn check_exists<T>(path: T) -> Option<PathBuf>
where
    T: Into<PathBuf>,
{
    let path = path.into();

    if path.exists() {
        Some(path)
    } else {
        None
    }
}
