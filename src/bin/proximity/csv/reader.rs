use std::io::Stdin;

use csv::{Reader, ReaderBuilder, StringRecord};
use geo::Point;
use quadtree::ToRadians;

use geo_munge::error::{Error, ParseType};
use geo_munge::qt::ParsedRecord;

use crate::args::Args;
use crate::InputSettings;

pub fn build_input_settings(args: &Args) -> Result<(Reader<Stdin>, InputSettings), Error> {
    // convert the delimiter into something useful for csv
    let delimiter = args.delimiter.as_bytes();
    if delimiter.len() != 1 {
        return Err(Error::InvalidDelimiter);
    }
    let delimiter = delimiter[0];

    // Set up the reader based on the passed input
    // Note that the reader must have headers that contain a lat and lng field,
    // and can contain an optional, configurable id field for subsequent matching
    let mut reader = ReaderBuilder::new()
        .has_headers(true)
        .delimiter(delimiter)
        .from_reader(std::io::stdin());

    // Get the label to look for the id
    // TODO: This should come from the args
    let id_label = None;
    let id_label = id_label.unwrap_or("id");

    let mut id_index = None;
    let mut lat_index = None;
    let mut lng_index = None;

    // Then look through the fields to find the id as well as the lng and lat fields
    for (i, label) in reader
        .headers()
        .map_err(|err| Error::CsvParseError(err))?
        .iter()
        .enumerate()
    {
        let label = label.to_lowercase();
        let label = label.as_str();

        if label == "id" {
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
            InputSettings {
                lat_index,
                lng_index,
                id_index,
                id_label,
                delimiter,
                // Drop in extra useful information from the args
                k: args.k,
                r: args.r,
                fields: args.fields.clone(),
                verbose: args.verbose,
            },
        ))
    } else {
        Err(Error::MissingLatLngField)
    }
}

pub fn parse_record<'a>(
    index: usize,
    record: Result<StringRecord, csv::Error>,
    settings: &InputSettings,
) -> Result<ParsedRecord, Error> {
    let record = record.map_err(|err| Error::CsvParseError(err))?;
    let lng = record
        .get(settings.lng_index)
        .unwrap()
        .parse::<f64>()
        .map_err(|_| Error::CannotParseRecord(index, ParseType::Lng))?;
    let lat = record
        .get(settings.lat_index)
        .unwrap()
        .parse::<f64>()
        .map_err(|_| Error::CannotParseRecord(index, ParseType::Lat))?;
    let id = settings
        .id_index
        .and_then(|i| Some(record.get(i).unwrap().to_owned()));

    let mut point = Point::new(lng, lat);
    point.to_radians_in_place();

    Ok(ParsedRecord {
        index,
        record,
        point,
        id,
    })
}
