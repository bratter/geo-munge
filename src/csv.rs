use std::io::{Stdin, Stdout};

use csv::{Reader, ReaderBuilder, StringRecord, Writer, WriterBuilder};
use geo::Point;
use quadtree::{Geometry, ToRadians, MEAN_EARTH_RADIUS};
use shapefile::dbase::{FieldValue, Record};

use crate::{error::FiberError, make_qt::IndexedDatum};

pub struct CsvSettings {
    pub lat_index: usize,
    pub lng_index: usize,
    pub id_index: Option<usize>,
    pub id_label: &'static str,
}

pub fn build_csv_settings(
    id_label: Option<&'static str>,
    delimiter: u8,
) -> Result<(Reader<Stdin>, CsvSettings), FiberError> {
    // Set up the reader based on the passed input
    // Note that the reader must have headers that contain a lat and lng field,
    // and can contain an optional, configurable id field for subsequent matching
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .delimiter(delimiter)
        .from_reader(std::io::stdin());

    // Get the label to look for the id
    let id_label = id_label.unwrap_or("id");

    let mut id_index = None;
    let mut lat_index = None;
    let mut lng_index = None;

    // TODO: Make a reasonable Error type/message here
    for (i, label) in reader
        .headers()
        .map_err(|_| {
            FiberError::IO(
                "cannot read csv input headers, please check the stdin input and try again",
            )
        })?
        .iter()
        .enumerate()
    {
        let label = label.to_lowercase();
        let label = label.as_str();

        if label == id_label {
            id_index = Some(i);
        } else if label == "lat" {
            lat_index = Some(i);
        } else if label == "lng" {
            lng_index = Some(i);
        }
    }

    // Settings are only valid if we have an index for both the lat and lng
    if let (Some(lat_index), Some(lng_index)) = (lat_index, lng_index) {
        Ok((
            reader,
            CsvSettings {
                lat_index,
                lng_index,
                id_index,
                id_label,
            },
        ))
    } else {
        Err(FiberError::IO("cannot find lng and lat fields in csv input headers"))
    }
}

pub struct ParsedRecord<'a> {
    pub record: &'a StringRecord,
    pub point: Point,
    pub id: Option<&'a str>,
}

pub fn parse_record<'a>(
    i: usize,
    record: Result<&'a StringRecord, &csv::Error>,
    settings: &CsvSettings,
) -> Result<ParsedRecord<'a>, FiberError<'a>> {
    let record = record.map_err(|_| FiberError::Parse(i, "cannot read csv input row"))?;
    let lng = record
        .get(settings.lng_index)
        .unwrap()
        .parse::<f64>()
        .map_err(|_| FiberError::Parse(i, "cannot parse lng for csv input row"))?;
    let lat = record
        .get(settings.lat_index)
        .unwrap()
        .parse::<f64>()
        .map_err(|_| FiberError::Parse(i, "cannot parse lat for csv input row"))?;
    let id = settings.id_index.and_then(|i| Some(record.get(i).unwrap()));

    let mut point = Point::new(lng, lat);
    point.to_radians_in_place();

    Ok(ParsedRecord { record, point, id })
}

// Output the header row with base and additional `--fields`. Will output the
// internal index of any matches and an `id` field, which will be balnk if it
// doesn't exist on the datum.
pub fn make_csv_writer<'a>(
    id_label: &str,
    delimiter: u8,
    fields: &Option<Vec<String>>,
) -> Result<Writer<Stdout>, FiberError<'a>> {
    let mut writer = WriterBuilder::new()
        .delimiter(delimiter)
        .from_writer(std::io::stdout());

    // Base fields to include in the output
    let base_fields = [
        id_label,
        "lng",
        "lat",
        "distance",
        "match_index",
        "match_id",
    ];

    // Set up the slice of additional fields to pull from the metdata
    // TODO: Can this be simplified? It seems like a lot of work
    let tmp_vec = Vec::new();
    let field_slice = fields
        .as_ref()
        .unwrap_or(&tmp_vec)
        .iter()
        .map(AsRef::as_ref);

    writer
        .write_record(base_fields.into_iter().chain(field_slice))
        .map_err(|_| FiberError::IO("cannot write header row to stdout"))?;

    Ok(writer)
}

pub struct WriteData<'a> {
    pub record: &'a StringRecord,
    pub datum: &'a IndexedDatum<Geometry<f64>>,
    pub meta: &'a Record,
    pub fields: &'a Option<Vec<String>>,
    pub dist: f64,
    pub id: Option<&'a str>,
    pub index: usize,
}

fn dbase_field_match(f: Option<&FieldValue>) -> String {
    match f {
        Some(FieldValue::Character(s)) => s.to_owned().unwrap_or(String::default()),
        Some(FieldValue::Integer(n)) => format!("{}", n),
        Some(FieldValue::Numeric(n)) => format!("{}", n.unwrap_or(f64::NAN)),
        _ => String::default(),
    }
}

// TODO: This works, but can we avoid allocating strings?
pub fn write_line(w: &mut Writer<Stdout>, settings: &CsvSettings, data: WriteData) {
    // If we parsed an id from the input data, then use it here
    // otherwise use the record's index as a unique id.
    // This is useful as errors mean you may not be able to just line up the
    // output with the input
    let id = if let Some(id) = data.id {
        id.to_string()
    } else {
        data.index.to_string()
    };

    // Convert the distance to meters and trucate at mm
    let dist = format!("{:.3}", data.dist * MEAN_EARTH_RADIUS);
    let match_index = format!("{}", data.datum.1);
    let match_id = dbase_field_match(data.meta.get("id"));

    // Make the base fields present in all output
    let base_fields = [
        id,
        data.record.get(settings.lng_index).unwrap().to_string(),
        data.record.get(settings.lat_index).unwrap().to_string(),
        dist,
        match_index,
        match_id,
    ];

    // Print the extra fields, or blank if they don't exist
    let tmp_vec = Vec::new();
    let meta_fields = data
        .fields
        .as_ref()
        .unwrap_or(&tmp_vec)
        .iter()
        .map(|f| dbase_field_match(data.meta.get(f)));

    if w.write_record(base_fields.into_iter().chain(meta_fields))
        .is_err()
    {
        eprintln!(
            "Failed to write output line for record at index {}.",
            data.index
        );
    }
}
