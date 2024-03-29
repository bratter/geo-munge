use std::io::Stdout;

use csv::{Writer, WriterBuilder};
use quadtree::MEAN_EARTH_RADIUS;

use geo_munge::error::Error;
use geo_munge::qt::{datum::Datum, ParsedRecord};

use crate::InputSettings;

// Output the header row with base and additional `--fields`. Will output the
// internal index of any matches and an `id` field, which will be balnk if it
// doesn't exist on the datum.
pub fn make_csv_writer<'a>(settings: &InputSettings) -> Result<Writer<Stdout>, Error> {
    let mut writer = WriterBuilder::new()
        .delimiter(settings.delimiter)
        .from_writer(std::io::stdout());

    // Base fields to include in the output
    let base_fields = [
        "input_index",
        &settings.id_label,
        "lng",
        "lat",
        "distance",
        "find_index",
    ];

    // Set up the slice of additional fields to pull from the metdata
    let tmp_vec = Vec::new();
    let field_slice = settings
        .fields
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
    pub datum: &'a Datum,
    pub distance: f64,
    pub parsed: &'a ParsedRecord,
    pub settings: &'a InputSettings,
}

pub fn write_line(w: &mut Writer<Stdout>, data: WriteData) {
    let WriteData {
        datum,
        distance,
        parsed,
        settings,
    } = data;

    // Make the base fields present in all output
    let base_fields = [
        // The index from the comparison point
        parsed.index.to_string(),
        // If we parsed an id from the input data, then use it here
        parsed.id.clone().unwrap_or_default(),
        // The lng from the csv input point
        parsed.record.get(settings.lng_index).unwrap().to_string(),
        // The lat from the csv input point
        parsed.record.get(settings.lat_index).unwrap().to_string(),
        // The closest distance to the returned datum, in meters, truncated at mm
        format!("{:.3}", distance * MEAN_EARTH_RADIUS),
        // The index of the found datum as recorded when the QuadTree was built
        // Aligns with the "find_index" column header
        datum.index().to_string(),
    ];

    let meta_iter = datum.meta_iter(&settings.fields);
    if w.write_record(base_fields.into_iter().chain(meta_iter))
        .is_err()
    {
        eprintln!(
            "Failed to write output line for record at index {}.",
            parsed.index
        );
    }
}
