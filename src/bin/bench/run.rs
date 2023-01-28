use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub enum RunGeometry {
    #[serde(alias = "point", alias = "POINT")]
    Point,

    #[serde(alias = "linestring", alias = "LINESTRING")]
    LineString,
}

impl ToString for RunGeometry {
    fn to_string(&self) -> String {
        let str = match self {
            Self::Point => "Point",
            Self::LineString => "LineString",
        };

        str.to_string()
    }
}

#[derive(Debug, Deserialize)]
pub struct Run {
    pub shape: RunGeometry,
    pub count: usize,
    pub meta: usize,
    pub cmp: usize,
}
