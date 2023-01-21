use std::io::Stdout;

use csv::{StringRecord, Writer, WriterBuilder};
use quadtree::MEAN_EARTH_RADIUS;

use super::reader::InputSettings;
use crate::error::Error;
use crate::qt::SearchResult;

// Output the header row with base and additional `--fields`. Will output the
// internal index of any matches and an `id` field, which will be balnk if it
// doesn't exist on the datum.
pub fn make_csv_writer<'a>(
    id_label: &str,
    delimiter: u8,
    fields: &Option<Vec<String>>,
) -> Result<Writer<Stdout>, Error> {
    let mut writer = WriterBuilder::new()
        .delimiter(delimiter)
        .from_writer(std::io::stdout());

    // Base fields to include in the output
    let base_fields = [id_label, "lng", "lat", "distance", "match_index"];

    // Set up the slice of additional fields to pull from the metdata
    let tmp_vec = Vec::new();
    let field_slice = fields
        .as_ref()
        .unwrap_or(&tmp_vec)
        .iter()
        .map(AsRef::as_ref);

    writer
        .write_record(base_fields.into_iter().chain(field_slice))
        .map_err(|err| Error::CsvWriteError(err))?;

    Ok(writer)
}

pub struct WriteData<'a> {
    pub result: SearchResult<'a>,
    pub record: &'a StringRecord,
    pub fields: &'a Option<Vec<String>>,
    pub id: &'a Option<String>,
    pub index: usize,
}

// TODO: Can we avoid allocating some of the strings?
pub fn write_line(w: &mut Writer<Stdout>, settings: &InputSettings, data: WriteData) {
    // If we parsed an id from the input data, then use it here
    // otherwise use the record's index as a unique id.
    // This is useful as errors mean you may not be able to just line up the
    // output with the input
    let id = if let Some(id) = data.id {
        id.to_string()
    } else {
        data.index.to_string()
    };

    let SearchResult {
        geom: _,
        index,
        meta,
        distance,
    } = data.result;

    // Convert the distance to meters and trucate at mm
    let dist = format!("{:.3}", distance * MEAN_EARTH_RADIUS);
    let match_index = format!("{}", index);

    // Make the base fields present in all output
    let base_fields = [
        id,
        data.record.get(settings.lng_index).unwrap().to_string(),
        data.record.get(settings.lat_index).unwrap().to_string(),
        dist,
        match_index,
    ];

    if w.write_record(base_fields.into_iter().chain(meta)).is_err() {
        eprintln!(
            "Failed to write output line for record at index {}.",
            data.index
        );
    }
}
