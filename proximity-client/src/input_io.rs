use std::{
    fs::File,
    io::{BufRead, BufReader, Lines, Read, Stdin},
    iter::Enumerate,
    path::Path,
};

use anyhow::{anyhow, bail, Result};
use encoding_rs_io::{DecodeReaderBytes, DecodeReaderBytesBuilder};
use protocol::prelude::*;

/// An abstraction layer over file or stdin input streams, useful for abstracting over input types in the client CLI.
pub enum Input {
    File(BufReader<DecodeReaderBytes<File, Vec<u8>>>),
    Stdin(BufReader<DecodeReaderBytes<Stdin, Vec<u8>>>),
}

impl Input {
    /// Make a new input abstraction.
    ///
    /// When [`None`] will attempt to use stdio, erroring if it is a tty rather than a pipe. Otherwise will attempt to
    /// open the provided path.
    pub fn try_new(path: Option<impl AsRef<Path>>) -> Result<Self> {
        if let Some(path) = path {
            Ok(Self::try_from(path.as_ref())?)
        } else {
            Self::stdin()
        }
    }

    /// Directly attempt to make an input abstraction from stdin. Will fail if stdin is a tty.
    pub fn stdin() -> Result<Self> {
        if atty::isnt(atty::Stream::Stdin) {
            let stdin = std::io::stdin();
            let decoder = DecodeReaderBytesBuilder::new()
                .bom_sniffing(true)
                .strip_bom(true)
                .build(stdin);
            Ok(Self::Stdin(BufReader::new(decoder)))
        } else {
            bail!("Attempted to pipe from stdin, but it is not a pipe.")
        }
    }

    /// Convert the Input into an iterator of line-oriented [`Feature`] types.
    ///
    /// Due to the highly variable size of a feature, we also return the length of the geojson string being processed to
    /// help as a proxy for batching. The geojson string will be longer than the binary encoding so will be a decent
    /// proxy for how large a request should be.
    pub fn into_feature_iter(self) -> impl Iterator<Item = Result<(usize, JsonFeature)>> {
        FeatureIterator::new(self)
    }

    /// Convert the Input into an iterator of line-oriented [`NodeId`] types.
    pub fn into_uid_iter(self) -> impl Iterator<Item = Result<Uid>> {
        self.lines().map(|line_result| {
            line_result
                .map_err(Into::into)
                .and_then(|line| line.parse::<Uid>().map_err(Into::into))
        })
    }

    /// Convert the Input into an iterator of line-oriented [`CustomKey`] types.
    /// TODO: Consider improving this as byte keys are always 16 bytes and may contain \n - perhaps the format here
    /// should just be 16 byte chunks (i.e., not line oriented at all)
    /// TODO: Could also implement more consistent error messages, similar to the FeatureIterator, on both of these
    pub fn into_custom_key_iter(self) -> impl Iterator<Item = Result<CustomKey>> {
        self.lines().map(|line_result| {
            line_result
                .map_err(Into::into)
                .and_then(|line| CustomKey::try_from(line.as_bytes()).map_err(Into::into))
        })
    }
}

impl TryFrom<&Path> for Input {
    type Error = std::io::Error;

    fn try_from(value: &Path) -> std::result::Result<Self, Self::Error> {
        let file = File::open(value)?;
        let decoder = DecodeReaderBytesBuilder::new()
            .bom_sniffing(true)
            .strip_bom(true)
            .build(file);
        let reader = BufReader::new(decoder);
        Ok(Input::File(reader))
    }
}

impl Read for Input {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Input::File(file) => file.read(buf),
            Input::Stdin(stdin) => stdin.read(buf),
        }
    }
}

impl BufRead for Input {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        match self {
            Input::File(file) => file.fill_buf(),
            Input::Stdin(stdin) => stdin.fill_buf(),
        }
    }

    fn consume(&mut self, amt: usize) {
        match self {
            Input::File(file) => file.consume(amt),
            Input::Stdin(stdin) => stdin.consume(amt),
        }
    }
}

struct FeatureIterator {
    inner: Enumerate<Lines<Input>>,
}

impl FeatureIterator {
    pub fn new(input: Input) -> Self {
        Self {
            inner: input.lines().enumerate(),
        }
    }
}

impl Iterator for FeatureIterator {
    type Item = Result<(usize, JsonFeature)>;

    // Reports all errors, including blank lines
    fn next(&mut self) -> Option<Self::Item> {
        if let Some((line_number, line_result)) = self.inner.next() {
            match line_result {
                // TODO: Are these really errors? Perhaps just in case, but should then have own error type
                Ok(s) if s.len() == 0 => Some(Err(anyhow!("Warning: Empty line {}", line_number))),
                Ok(s) => match s.parse::<JsonFeature>() {
                    Ok(f) => Some(Ok((s.len(), f))),
                    Err(err) => Some(Err(anyhow!(
                        "Warning: Could not parse line {}: {}",
                        line_number,
                        err
                    ))),
                },
                Err(err) => Some(Err(anyhow!(
                    "Warning: Could not read line {}: {}",
                    line_number,
                    err
                ))),
            }
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    static MANIFEST: &str = env!("CARGO_MANIFEST_DIR");

    fn get_name<'a>((_, f): &'a (usize, JsonFeature)) -> &'a str {
        f.0.properties.as_ref().unwrap()["name"].as_str().unwrap()
    }

    #[test]
    fn test_utf8_multiline_file() {
        let path = Path::new(MANIFEST).join("../data/io_test/utf8_multiline.json");
        let input = Input::try_from(path.as_path()).unwrap();
        let features: Result<Vec<_>> = input.into_feature_iter().collect();
        let features = features.unwrap();

        assert_eq!(features.len(), 4);
        assert_eq!(get_name(&features[0]), "San Francisco");
        assert_eq!(get_name(&features[1]), "New York");
        assert_eq!(get_name(&features[2]), "Paris");
        assert_eq!(get_name(&features[3]), "Tokyo");
    }

    #[test]
    fn test_utf8_with_bom_file() {
        let path = Path::new(MANIFEST).join("../data/io_test/utf8_with_bom.json");
        let input = Input::try_from(path.as_path()).unwrap();
        let features: Result<Vec<_>> = input.into_feature_iter().collect();
        let features = features.unwrap();

        assert_eq!(features.len(), 1);
        assert_eq!(get_name(&features[0]), "San Francisco");
    }

    #[test]
    fn test_utf16le_file() {
        let path = Path::new(MANIFEST).join("../data/io_test/utf16le_with_bom.json");
        let input = Input::try_from(path.as_path()).unwrap();
        let features: Result<Vec<_>> = input.into_feature_iter().collect();
        let features = features.unwrap();

        assert_eq!(features.len(), 1);
        assert_eq!(get_name(&features[0]), "Test UTF-16");
    }
}
