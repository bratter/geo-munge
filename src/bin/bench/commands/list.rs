use std::{fs, path::PathBuf};

use csv::Writer;

use geo_munge::error::Error;

use crate::run::RunSet;

/// Outputs a list of currently stored runs to stdout. As this has to parse all the files it is
/// relatively resource intensive.
pub fn list(run_path: &PathBuf) -> Result<(), Error> {
    let dir = fs::read_dir(run_path)
        .map_err(|err| Error::FileIOError(err))?
        // Here we drop anything in the listing that errors or is not a file
        // TODO: Consider outputting to stderr here inside the filter_maps
        .filter_map(|result| result.ok())
        .filter_map(|entry| match entry.file_type() {
            Ok(ft) if ft.is_file() == true => Some(entry.path()),
            _ => None,
        })
        .map(|path| Ok::<_, Error>(RunSet::try_from(&path)?));

    // Create the csv writer then write the header row
    let mut writer = Writer::from_writer(std::io::stdout());
    writer
        .write_record(&["file", "index", "shape", "count", "meta", "cmp"])
        .map_err(|err| Error::CsvWriteError(err))?;

    for run_res in dir {
        match run_res {
            Ok(run_set) => {
                for run in run_set {
                    let run_data = [
                        run.name.to_string(),
                        run.index.to_string(),
                        run.shape.to_string(),
                        run.count.to_string(),
                        run.meta.to_string(),
                        run.cmp.to_string(),
                    ];

                    if writer.write_record(&run_data).is_err() {
                        eprintln!(
                            "Writing record at index {} for file {} failed",
                            run.index, run.name
                        );
                    }
                }
            }
            // Print file read or deserialization errors
            Err(err) => eprintln!("{err}"),
        }
    }

    Ok(())
}
