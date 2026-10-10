//! Error type for the crate.

use std::fmt;

/// Errors that can occur while knitting Markdown into a `.docx`.
#[derive(Debug)]
pub enum Error {
    /// An I/O error (reading the Markdown source or writing the `.docx`).
    Io(std::io::Error),
    /// The `docx-rs` writer failed while packing the document (zip layer).
    Pack(String),
    /// A Markdown input file had no readable content / wrong extension, etc.
    Input(String),
    /// A theme/config file could not be parsed, or held an invalid value
    /// (bad hex colour, wrong number of heading sizes, unknown page size, …).
    Config(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::Pack(e) => write!(f, "failed to pack .docx: {e}"),
            Error::Input(e) => write!(f, "invalid input: {e}"),
            Error::Config(e) => write!(f, "invalid config: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;
