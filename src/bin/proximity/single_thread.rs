use std::time::Instant;

use geo_munge::csv::reader::{parse_record, InputSettings};
use geo_munge::csv::writer::{write_line, WriteData};
use geo_munge::qt::Quadtree;

use crate::args::Args;

pub(super) struct SingleThreadOptions {
    pub qt: Quadtree,
    pub csv_reader: csv::Reader<std::io::Stdin>,
    pub csv_writer: csv::Writer<std::io::Stdout>,
    pub args: Args,
    pub settings: InputSettings,
}

pub(super) fn exec_single_thread(opts: SingleThreadOptions) {
    let SingleThreadOptions {
        qt,
        mut csv_reader,
        mut csv_writer,
        args,
        settings,
    } = opts;

    if args.verbose {
        eprintln!("Starting single-threaded execution");
    }
    let start = Instant::now();

    for (i, record) in csv_reader.records().enumerate() {
        match (parse_record(i, record, &settings), args.k) {
            (Ok(parsed), None) | (Ok(parsed), Some(1)) => {
                match qt.find(&parsed, args.r, &args.fields) {
                    Ok(result) => {
                        let data = WriteData {
                            result,
                            record: &parsed.record,
                            fields: &args.fields,
                            id: &parsed.id,
                            index: i,
                        };

                        write_line(&mut csv_writer, &settings, data);
                    }
                    Err(err) => eprintln!("{err}"),
                }
            }
            (Ok(parsed), Some(k)) => match qt.knn(&parsed, k, args.r, &args.fields) {
                Ok(results) => {
                    for result in results {
                        let data = WriteData {
                            result,
                            record: &parsed.record,
                            fields: &args.fields,
                            id: &parsed.id,
                            index: i,
                        };

                        write_line(&mut csv_writer, &settings, data);
                    }
                }
                Err(err) => eprintln!("{err}"),
            },
            (Err(err), _) => eprintln!("{err}"),
        }

        if args.verbose && i % 10000 == 0 {
            eprintln!(
                "Processed {} records in {} ms",
                i,
                start.elapsed().as_millis()
            );
        }
    }
}
