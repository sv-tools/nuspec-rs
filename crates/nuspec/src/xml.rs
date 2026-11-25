use crate::{Package, to_string_indent};
use std::{error, fmt, io, str::FromStr};

/// The XML declaration written at the top of every `.nuspec` file.
pub const XML_DECLARATION: &str = r#"<?xml version="1.0" encoding="utf-8"?>"#;

/// An error reading or writing a `.nuspec` document.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The document is not a valid nuspec.
    Deserialize(quick_xml::DeError),
    /// The package could not be serialized to XML.
    Serialize(quick_xml::SeError),
    /// Reading or writing the document failed.
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Deserialize(e) => write!(f, "failed to parse nuspec: {e}"),
            Error::Serialize(e) => write!(f, "failed to serialize nuspec: {e}"),
            Error::Io(e) => write!(f, "nuspec I/O error: {e}"),
        }
    }
}

// The messages include the underlying errors, so they are not also reported as sources:
// error reporters that print the chain would show them twice.
impl error::Error for Error {}

impl From<quick_xml::DeError> for Error {
    fn from(e: quick_xml::DeError) -> Self {
        Error::Deserialize(e)
    }
}

impl From<quick_xml::SeError> for Error {
    fn from(e: quick_xml::SeError) -> Self {
        Error::Serialize(e)
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl Package {
    /// Parses a `.nuspec` document from a string.
    pub fn from_xml(xml: &str) -> Result<Self, Error> {
        Ok(quick_xml::de::from_str(xml)?)
    }

    /// Parses a `.nuspec` document from a buffered reader, such as an opened file.
    pub fn from_reader<R: io::BufRead>(reader: R) -> Result<Self, Error> {
        Ok(quick_xml::de::from_reader(reader)?)
    }

    /// Serializes the package as a complete `.nuspec` document: the XML declaration followed by
    /// the `<package>` element indented with two spaces, ending with a newline.
    pub fn to_xml(&self) -> Result<String, Error> {
        let body = to_string_indent(self, ' ', 2)?;
        let mut xml = String::with_capacity(XML_DECLARATION.len() + body.len() + 2);
        xml.push_str(XML_DECLARATION);
        xml.push('\n');
        xml.push_str(&body);
        xml.push('\n');
        Ok(xml)
    }

    /// Writes the package as a complete `.nuspec` document, see [`Package::to_xml`].
    pub fn write_to<W: io::Write>(&self, mut writer: W) -> Result<(), Error> {
        writer.write_all(self.to_xml()?.as_bytes())?;
        writer.flush()?;
        Ok(())
    }
}

impl FromStr for Package {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_xml(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Metadata, NUSPEC_NAMESPACE};

    fn package() -> Package {
        Package {
            metadata: Metadata {
                id: "example".to_string(),
                version: "1.0.0".to_string(),
                description: "Example".to_string(),
                authors: vec!["Author".to_string()],
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn test_to_xml() {
        let expected = format!(
            "{XML_DECLARATION}\n<package xmlns=\"{NUSPEC_NAMESPACE}\">\n  <metadata>\n    <id>example</id>\n    <version>1.0.0</version>\n    <description>Example</description>\n    <authors>Author</authors>\n  </metadata>\n</package>\n"
        );
        assert_eq!(package().to_xml().unwrap(), expected);

        let mut buf = Vec::new();
        package().write_to(&mut buf).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), expected);
    }

    #[test]
    fn test_from_xml_roundtrip() {
        let xml = package().to_xml().unwrap();
        assert_eq!(Package::from_xml(&xml).unwrap(), package());
        assert_eq!(xml.parse::<Package>().unwrap(), package());
        assert_eq!(Package::from_reader(xml.as_bytes()).unwrap(), package());
    }

    #[test]
    fn test_from_xml_error() {
        let err = Package::from_xml("<package><unknown/></package>").unwrap_err();
        assert!(matches!(err, Error::Deserialize(_)), "{err:?}");
        assert!(err.to_string().starts_with("failed to parse nuspec"));
    }
}
