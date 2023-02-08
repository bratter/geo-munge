use geo_munge::qt::Quadtree;

use crate::{
    run::{run_find, run_output},
    CsvReader, CsvWriter, InputSettings,
};

pub(super) fn exec_single_thread(
    mut csv_reader: CsvReader,
    mut csv_writer: CsvWriter,
    qt: &Quadtree,
    settings: &InputSettings,
) {
    csv_reader.records().enumerate().for_each(|enum_record| {
        let output = run_find(enum_record, &qt, &settings);
        run_output(&mut csv_writer, &settings, output);
    });
}
