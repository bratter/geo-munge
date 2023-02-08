use geo_munge::error::Error;
use geo_munge::qt::{ParsedRecord, Quadtree, SearchResult};

use crate::csv::reader::parse_record;
use crate::csv::writer::{write_line, WriteData};
use crate::{CsvWriter, InputSettings};

pub(crate) type EnumeratedRecord = (usize, Result<csv::StringRecord, csv::Error>);

pub(crate) enum FindResult<'a> {
    One(ParsedRecord, SearchResult<'a>),
    Many(ParsedRecord, Vec<SearchResult<'a>>),
}

/// Calculates matches in the quadtree from the provided record.
///
/// Here we determine the result or pass through/add any errors. This function needs to have the
/// ability to be parallelized, so must be thread safe.
pub(crate) fn run_find<'a>(
    enum_record: EnumeratedRecord,
    qt: &'a Quadtree,
    settings: &InputSettings,
) -> Result<FindResult<'a>, Error> {
    let (csv_idx, record) = enum_record;
    match (parse_record(csv_idx, record, &settings)?, settings.k) {
        (parsed, None) | (parsed, Some(1)) => {
            let results = qt.find(&parsed, settings.r)?;
            Ok(FindResult::One(parsed, results))
        }
        (parsed, Some(k)) => {
            let results = qt.knn(&parsed, k, settings.r)?;
            Ok(FindResult::Many(parsed, results))
        }
    }
}

/// Outputs the result of a find/knn.
///
/// If successful, prints matching records to stdout using the csv writer. If the find failed,
/// outputs the error to stderr using eprintln. Done separately from the find itself so the find
/// can be parallelized without having to deal with the mutable writer reference.
pub(crate) fn run_output(
    writer: &mut CsvWriter,
    settings: &InputSettings,
    output: Result<FindResult, Error>,
) {
    match output {
        Ok(FindResult::One(ref parsed, (datum, distance))) => {
            write_line(
                writer,
                WriteData {
                    datum,
                    distance,
                    parsed,
                    settings,
                },
            );
        }
        Ok(FindResult::Many(ref parsed, results)) => {
            for (datum, distance) in results {
                write_line(
                    writer,
                    WriteData {
                        datum,
                        distance,
                        parsed,
                        settings,
                    },
                );
            }
        }
        Err(err) => eprintln!("{err}"),
    }
}
