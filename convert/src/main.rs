//! Geo-munge convert binary.
//!
//! Convert between basic GIS formats, optionally preserving metadata.

mod args;
mod format;
mod stream;

use anyhow::Result;

use args::Cli;
use format::InputSpec;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd)]
enum QuietLevel {
    Normal,
    NoErrors,
    NoMessages,
}

impl From<u8> for QuietLevel {
    fn from(value: u8) -> Self {
        match value {
            0 => QuietLevel::Normal,
            1 => QuietLevel::NoErrors,
            2 => QuietLevel::NoMessages,
            _ => unreachable!("Clap restricts this to 0–2"),
        }
    }
}

fn main() -> Result<()> {
    let args = Cli::parse();
    let quiet = args.quiet;

    if quiet < QuietLevel::NoMessages {
        let input_fmt = match &args.input {
            InputSpec::Streamable { format, .. } => format!("{}", format),
            InputSpec::FileOnly { format, .. } => format!("{}", format),
        };
        eprintln!(
            "Starting to process, converting {} to {}",
            input_fmt, args.output.format
        );
    }

    // TODO: Better exit?
    let (ok_chunks, err_chunks) = run(args)?;

    if quiet < QuietLevel::NoMessages {
        eprintln!(
            "\nProcessing complete, emitted {} chunks with {} errors",
            ok_chunks, err_chunks
        );
        eprintln!("Note that chunks do not map 1:1 with emitted shapes",);
    }

    Ok(())
}

fn run(args: Cli) -> Result<(usize, usize)> {
    // TODO: Any other transforms, flattens, filters, etc. can be introduced in between the reader and the output
    // transformer as long as they are GeoItemIterators
    // TODO: This should have the option to flatten if not done in the reader
    // TODO: Also want a seek setting to pre-pull fields for unstructured metadata formats like json
    // TODO: Some form of permissiveness control that decides when to log vs. abort

    // Prepare the reader and writer
    let reader = args.input.create_reader(args.mode)?;
    let transformer = args
        .output
        .create_transformer(reader, args.mode, args.csv_settings)?;
    let writer = args.output.create_writer(transformer, args.quiet)?;

    // Drive the writer
    let mut ok_chunks = 0;
    let mut err_chunks = 0;
    for res in writer {
        match res {
            Ok(_) => ok_chunks += 1,
            Err(_) => err_chunks += 1,
        }
    }

    Ok((ok_chunks, err_chunks))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::format::{
        csv::{CsvGeom, CsvSettings},
        ContentMode,
    };
    use format::{OutputSpec, StreamableFormat};

    const JSON: &'static str = r#"
      {
        type: "FeatureCollection",
        features: [
          {
            "type": "Feature",
            "geometry": {
              "type": "Point",
              "coordinates": [1.1, 1.2]
            },
            "properties": { "x": 1 }
          },
          {
            "type": "Feature",
            "geometry": {
              "type": "Point",
              "coordinates": [2.1, 2.2]
            },
            "properties": { }
          }
        ]
      }
    "#;

    #[test]
    fn convert_geojson_to_ndjson() {
        let output = Arc::new(Mutex::new(String::new()));
        let args = Cli {
            input: InputSpec::with_str(JSON.to_string(), StreamableFormat::JsonStream),
            output: OutputSpec::with_str(Arc::clone(&output), StreamableFormat::Ndjson),
            mode: ContentMode::Full,
            csv_settings: CsvSettings::default(),
            quiet: QuietLevel::Normal,
        };

        let (ok_chunks, err_chunks) = run(args).unwrap();
        assert_eq!(ok_chunks, 2);
        assert_eq!(err_chunks, 0);

        // Parse and validate each line as GeoJSON
        let output_str = output.lock().unwrap();
        let lines: Vec<&str> = output_str.trim().split('\n').collect();
        assert_eq!(lines.len(), 2);

        // Validate first feature
        let feature1: geojson::Feature = serde_json::from_str(lines[0]).unwrap();
        let geom1 = feature1.geometry.unwrap();
        if let geojson::Value::Point(coords) = &geom1.value {
            assert_eq!(coords[0], 1.1);
            assert_eq!(coords[1], 1.2);
        }
        let props1 = feature1.properties.unwrap();
        assert_eq!(
            props1.get("x").unwrap(),
            &serde_json::Value::Number(serde_json::Number::from(1))
        );

        // Validate second feature
        let feature2: geojson::Feature = serde_json::from_str(lines[1]).unwrap();
        let geom2 = feature2.geometry.unwrap();
        if let geojson::Value::Point(coords) = &geom2.value {
            assert_eq!(coords[0], 2.1);
            assert_eq!(coords[1], 2.2);
        }
        let props2 = feature2.properties.unwrap();
        assert!(props2.is_empty());
    }

    #[test]
    fn convert_geojson_to_ndjson_geometry_only() {
        let output = Arc::new(Mutex::new(String::new()));
        let args = Cli {
            input: InputSpec::with_str(JSON.to_string(), StreamableFormat::JsonStream),
            output: OutputSpec::with_str(Arc::clone(&output), StreamableFormat::Ndjson),
            mode: ContentMode::Geometry,
            csv_settings: CsvSettings::default(),
            quiet: QuietLevel::Normal,
        };

        let (ok_chunks, err_chunks) = run(args).unwrap();
        assert_eq!(ok_chunks, 2);
        assert_eq!(err_chunks, 0);

        // Parse and validate each line as GeoJSON
        let output_str = output.lock().unwrap();
        let lines: Vec<&str> = output_str.trim().split('\n').collect();
        assert_eq!(lines.len(), 2);

        // Validate first feature has geometry but no properties
        let feature1: geojson::Feature = serde_json::from_str(lines[0]).unwrap();
        let geom1 = feature1.geometry.unwrap();
        if let geojson::Value::Point(coords) = &geom1.value {
            assert_eq!(coords[0], 1.1);
            assert_eq!(coords[1], 1.2);
        }
        assert!(feature1.properties.is_none());

        // Validate second feature has geometry but no properties
        let feature2: geojson::Feature = serde_json::from_str(lines[1]).unwrap();
        let geom2 = feature2.geometry.unwrap();
        if let geojson::Value::Point(coords) = &geom2.value {
            assert_eq!(coords[0], 2.1);
            assert_eq!(coords[1], 2.2);
        }
        assert!(feature2.properties.is_none());
    }

    #[test]
    fn convert_geojson_to_ndjson_properties_only() {
        let output = Arc::new(Mutex::new(String::new()));
        let args = Cli {
            input: InputSpec::with_str(JSON.to_string(), StreamableFormat::JsonStream),
            output: OutputSpec::with_str(Arc::clone(&output), StreamableFormat::Ndjson),
            mode: ContentMode::Properties,
            csv_settings: CsvSettings::default(),
            quiet: QuietLevel::Normal,
        };

        let (ok_chunks, err_chunks) = run(args).unwrap();
        assert_eq!(ok_chunks, 2);
        assert_eq!(err_chunks, 0);

        // Parse and validate each line as JSON objects
        let output_str = output.lock().unwrap();
        let lines: Vec<&str> = output_str.trim().split('\n').collect();
        assert_eq!(lines.len(), 2);

        // Validate first object has x == 1
        let obj1: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(
            obj1.get("x").unwrap(),
            &serde_json::Value::Number(serde_json::Number::from(1))
        );

        // Validate second object is empty
        let obj2: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert!(obj2.as_object().unwrap().is_empty());
    }

    #[test]
    fn convert_geojson_to_csv_with_custom_settings() {
        let output = Arc::new(Mutex::new(String::new()));
        let args = Cli {
            input: InputSpec::with_str(JSON.to_string(), StreamableFormat::JsonStream),
            output: OutputSpec::with_str(Arc::clone(&output), StreamableFormat::Csv),
            mode: ContentMode::Full,
            csv_settings: CsvSettings {
                geom: CsvGeom::LngLat("lng".to_string(), "lat".to_string()),
                delimiter: b'|',
            },
            quiet: QuietLevel::Normal,
        };

        let (ok_chunks, err_chunks) = run(args).unwrap();
        assert_eq!(ok_chunks, 3); // Header + 2 data rows
        assert_eq!(err_chunks, 0);

        // Validate exact CSV output
        let output_str = output.lock().unwrap();
        let lines: Vec<&str> = output_str.trim().split('\n').collect();
        assert_eq!(lines.len(), 3);

        assert_eq!(lines[0], "lng|lat|x");
        assert_eq!(lines[1], "1.1|1.2|1");
        assert_eq!(lines[2], "2.1|2.2|");
    }
}
