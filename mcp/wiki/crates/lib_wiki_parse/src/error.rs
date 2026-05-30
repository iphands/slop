//! Error types for Wikipedia parsing

use lib_common::WikiParseError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("XML parsing error: {0}")]
    XmlError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Encoding error: {0}")]
    EncodingError(String),
}

impl From<quick_xml::Error> for ParseError {
    fn from(err: quick_xml::Error) -> Self {
        ParseError::XmlError(err.to_string())
    }
}

impl From<ParseError> for WikiParseError {
    fn from(err: ParseError) -> Self {
        match err {
            ParseError::XmlError(msg) => WikiParseError::XmlError(msg),
            ParseError::IoError(e) => WikiParseError::IoError(e),
            ParseError::EncodingError(msg) => WikiParseError::EncodingError(msg),
        }
    }
}
