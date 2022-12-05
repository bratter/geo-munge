use std::{collections::HashMap, fmt::Display, fs::read_to_string, iter::empty, path::PathBuf};

use csv::WriterBuilder;
use geo_munge::error::FiberError;
use geojson::{feature::Id, FeatureCollection, GeoJson, JsonValue};

use crate::{DataOpts, Meta, MetaResult};

pub struct GeoJsonMeta {
    path: PathBuf,
}

impl GeoJsonMeta {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn geojson(&self) -> Result<GeoJson, FiberError> {
        let geojson = read_to_string(&self.path)
            .map_err(|_| FiberError::IO("Cannot read GeoJson file"))?
            .parse::<GeoJson>()
            .map_err(|_| FiberError::IO("Cannot parse GeoJson file"))?;

        Ok(geojson)
    }
}

fn print_bbox(bbox: &Option<Vec<f64>>) -> MetaResult {
    if let Some(bbox) = bbox {
        let str = "Bounding box present but invalid";

        println!(
            "Bounding box: [{}, {}, {}, {}]",
            bbox.get(0).ok_or(FiberError::Parse(0, str))?,
            bbox.get(1).ok_or(FiberError::Parse(0, str))?,
            bbox.get(2).ok_or(FiberError::Parse(0, str))?,
            bbox.get(3).ok_or(FiberError::Parse(0, str))?
        );
    }

    Ok(())
}

impl Meta for GeoJsonMeta {
    fn headers(&self) -> MetaResult {
        let geojson = self.geojson()?;

        match geojson {
            GeoJson::FeatureCollection(fc) => {
                println!("Top-level type: FeatureCollection");
                print_bbox(&fc.bbox)?;
            }
            GeoJson::Feature(f) => {
                println!("Top-level type: Feature");
                match f.geometry {
                    Some(g) => println!("Contained geometry: {}", g.value.type_name()),
                    None => println!("Contained geometry: None"),
                };
                print_bbox(&f.bbox)?;
            }
            GeoJson::Geometry(g) => {
                println!("Top-level type: Geometry");
                println!("Contained geometry: {}", g.value.type_name());
                print_bbox(&g.bbox)?;
            }
        };

        Ok(())
    }

    /// Print a list of metadata fields to stdout.
    ///
    /// For GeoJson FeatureCollections, will print id (as its always possible),
    /// then iterate through all the Features, capturing the first level of the
    /// properties key. Feature will just return the top-level properties of the
    /// Feature, and Geometry will error.
    fn fields(&self, show_types: bool) -> MetaResult {
        match self.geojson()? {
            GeoJson::Geometry(_) => {
                Err(FiberError::Parse(
                    0,
                    "GeoJson Geometry types do not contain metadata",
                ))?;
            }
            GeoJson::Feature(f) => {
                // The id, if it exists, can be a string or a number
                if show_types {
                    match f.id {
                        Some(Id::String(_)) => println!("id [String]"),
                        Some(Id::Number(_)) => println!("id [Number]"),
                        None => {}
                    }
                } else if f.id.is_some() {
                    println!("id");
                }

                // If the properties key exists, loop through and print
                if let Some(props) = f.properties {
                    for (k, v) in props {
                        if show_types {
                            println!("{k} [{}]", json_type(&v));
                        } else {
                            println!("{k}");
                        }
                    }
                }
            }
            GeoJson::FeatureCollection(fc) => {
                // Eagerly loop through the collection to determine all metadata
                // keys - this can be time consuming.
                let (id_type, keys) = make_fields(&fc, None, None);

                if id_type != IdType::None {
                    if show_types {
                        println!("id [{id_type}]");
                    } else {
                        println!("id");
                    }
                }

                for (k, t) in keys {
                    if show_types {
                        println!("{k} [{t}]");
                    } else {
                        println!("{k}");
                    }
                }
            }
        };

        Ok(())
    }

    /// Print to number of top-level records.
    ///
    /// For GeoJson this will be 1 for Feature or Geometry types, or the length
    /// of the Features vector for FeatureCollections.
    fn count(&self) -> MetaResult {
        let geojson = self.geojson()?;

        match geojson {
            GeoJson::Feature(_) | GeoJson::Geometry(_) => println!("{}", 1),
            GeoJson::FeatureCollection(fc) => println!("{}", fc.features.len()),
        }

        Ok(())
    }

    fn data(&self, opts: DataOpts) -> MetaResult {
        match self.geojson()? {
            GeoJson::Geometry(_) => {
                Err(FiberError::Parse(
                    0,
                    "GeoJson Geometry types do not contain metadata",
                ))?;
            }
            GeoJson::Feature(f) => {
                let fc = FeatureCollection {
                    bbox: None,
                    foreign_members: None,
                    features: vec![f],
                };
                print_fc_meta(fc, opts)?;
            }
            GeoJson::FeatureCollection(fc) => {
                print_fc_meta(fc, opts)?;
            }
        }

        Ok(())
    }
}

#[derive(PartialEq, Clone, Copy)]
enum IdType {
    String,
    Number,
    Mixed,
    None,
}

impl Display for IdType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            IdType::None => "",
            IdType::String => "String",
            IdType::Number => "Number",
            IdType::Mixed => "Mixed",
        };

        write!(f, "{}", str)
    }
}

fn make_fields<'a>(
    fc: &'a FeatureCollection,
    skip: Option<usize>,
    n: Option<usize>,
) -> (IdType, HashMap<String, &'static str>) {
    let mut id_type = IdType::None;
    let mut keys = HashMap::new();

    for f in fc
        .into_iter()
        .skip(skip.unwrap_or(0))
        .take(n.unwrap_or(usize::MAX))
    {
        let t = match f.id {
            Some(Id::String(_)) => IdType::String,
            Some(Id::Number(_)) => IdType::Number,
            None => IdType::None,
        };

        if id_type == IdType::None {
            id_type = t;
        } else if id_type != IdType::Mixed && id_type != t {
            id_type = IdType::Mixed;
        }

        if let Some(ref props) = f.properties {
            for (k, v) in props {
                let t = json_type(v);
                keys.entry(k.to_owned())
                    .and_modify(|e| {
                        if *e != t {
                            *e = "Mixed"
                        }
                    })
                    .or_insert(t);
            }
        }
    }

    (id_type, keys)
}

/// Return a string representation of a Json type.
fn json_type(v: &JsonValue) -> &'static str {
    match v {
        JsonValue::Null => "Null",
        JsonValue::Bool(_) => "Bool",
        JsonValue::Number(_) => "Number",
        JsonValue::String(_) => "String",
        JsonValue::Array(_) => "Array",
        JsonValue::Object(_) => "Object",
    }
}

/// Convert a JsonValue to a string representation.
/// Required in this application to avoid double escaping strings when
/// converting for csv.
fn json_value_to_string(v: Option<&JsonValue>) -> String {
    match v {
        Some(JsonValue::String(s)) => s.to_owned(),
        Some(x) => x.to_string(),
        None => String::default(),
    }
}

fn print_fc_meta(fc: FeatureCollection, opts: DataOpts) -> MetaResult {
    let delimiter = opts.delimiter.as_bytes();
    if delimiter.len() != 1 {
        return Err(Box::new(FiberError::Arg("Invalid delimiter provided")));
    }
    let delimiter = delimiter[0];

    let mut writer = WriterBuilder::new()
        .delimiter(delimiter)
        .from_writer(std::io::stdout());

    // TODO: Different size parameter for the length to scan for headers?
    let (has_id, keys) = make_fields(&fc, Some(opts.start), opts.length);
    let keys: Vec<_> = keys.keys().collect();

    // Write out the header
    if opts.headers {
        if opts.index {
            writer.write_field("index")?;
        }
        if has_id != IdType::None {
            writer.write_field("id")?;
        }
        for k in &keys {
            writer.write_field(k)?;
        }
        writer.write_record(empty::<&str>())?;
    }

    for (i, f) in fc
        .into_iter()
        .skip(opts.start)
        .take(opts.length.unwrap_or(usize::MAX))
        .enumerate()
    {
        if opts.index {
            writer.write_field(i.to_string())?;
        }
        if has_id != IdType::None {
            match f.id {
                Some(Id::String(s)) => writer.write_field(s)?,
                Some(Id::Number(n)) => writer.write_field(n.to_string())?,
                _ => {}
            }
        }
        let props = f.properties.unwrap_or_default();
        writer.write_record(keys.iter().map(|k| json_value_to_string(props.get(*k))))?;
    }

    Ok(())
}
