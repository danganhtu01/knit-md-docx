//! # rust_knit_md_docx
//!
//! Knit Markdown — CommonMark plus the GitHub Flavored Markdown extensions —
//! into a Microsoft Word `.docx` file, aiming for high visual fidelity.
//!
//! The crate pairs [`pulldown-cmark`](https://docs.rs/pulldown-cmark) (parser)
//! with [`docx-rs`](https://docs.rs/docx-rs) (writer). It supports headings,
//! emphasis/strong/strikethrough/inline-code, links (external + intra-document
//! anchors), images, ordered/unordered/nested/task lists, fenced code blocks,
//! block quotes and GFM alerts, tables with per-column alignment, thematic
//! breaks, footnotes, definition lists, a best-effort subset of inline HTML,
//! and YAML front matter.
//!
//! ## Quick start
//!
//! ```no_run
//! // Convert a file on disk, resolving relative image paths next to it.
//! rust_knit_md_docx::convert_file("README.md", "README.docx").unwrap();
//! ```
//!
//! ```
//! // Convert a string to an in-memory `.docx` byte buffer.
//! let bytes = rust_knit_md_docx::to_bytes("# Hello\n\nWorld **bold**.").unwrap();
//! assert_eq!(&bytes[..2], b"PK"); // it's a zip
//! ```
//!
//! ```
//! // Customise the conversion.
//! use rust_knit_md_docx::{Converter, ConvertOptions, PageSetup};
//!
//! let mut opts = ConvertOptions::default();
//! opts.page = PageSetup::A4;
//! opts.smart_punctuation = true;
//! let docx = Converter::with_options(opts).to_docx("Text -- with smart dashes.");
//! let _ = docx; // a `docx_rs::Docx` you can further customise and pack yourself
//! ```

mod engine;
mod error;
mod options;
mod styles;

pub use docx_rs;
pub use docx_rs::Docx;
pub use error::{Error, Result};
pub use options::{ConvertOptions, PageSetup};

use std::io::Cursor;
use std::path::Path;

/// Convert Markdown to a [`Docx`] using the default options.
///
/// The returned document can be further edited and then packed via
/// `docx.build().pack(writer)`.
pub fn to_docx(markdown: &str) -> Docx {
    engine::build_docx(markdown, &ConvertOptions::default())
}

/// Convert Markdown to a [`Docx`] using the supplied options.
pub fn to_docx_with(markdown: &str, opts: &ConvertOptions) -> Docx {
    engine::build_docx(markdown, opts)
}

/// Convert Markdown to a `.docx` byte buffer (default options).
pub fn to_bytes(markdown: &str) -> Result<Vec<u8>> {
    to_bytes_with(markdown, &ConvertOptions::default())
}

/// Convert Markdown to a `.docx` byte buffer using the supplied options.
pub fn to_bytes_with(markdown: &str, opts: &ConvertOptions) -> Result<Vec<u8>> {
    let docx = engine::build_docx(markdown, opts);
    // `pack` needs `Write + Seek`; a bare `Vec<u8>` is not `Seek`, a `Cursor` is.
    let mut cur = Cursor::new(Vec::new());
    docx.build()
        .pack(&mut cur)
        .map_err(|e| Error::Pack(e.to_string()))?;
    Ok(cur.into_inner())
}

/// Convert a Markdown string and write the `.docx` to `output` (default options).
pub fn write_file(markdown: &str, output: impl AsRef<Path>) -> Result<()> {
    write_file_with(markdown, &ConvertOptions::default(), output)
}

/// Convert a Markdown string and write the `.docx` to `output` (custom options).
pub fn write_file_with(
    markdown: &str,
    opts: &ConvertOptions,
    output: impl AsRef<Path>,
) -> Result<()> {
    let docx = engine::build_docx(markdown, opts);
    let file = std::fs::File::create(output)?;
    docx.build()
        .pack(file)
        .map_err(|e| Error::Pack(e.to_string()))?;
    Ok(())
}

/// Read a Markdown file and write a `.docx` (default options).
///
/// Relative image paths in the Markdown are resolved against the input file's
/// directory unless [`ConvertOptions::base_dir`] is set explicitly.
pub fn convert_file(input: impl AsRef<Path>, output: impl AsRef<Path>) -> Result<()> {
    Converter::new().convert_file(input, output)
}

/// A reusable converter holding a set of [`ConvertOptions`].
#[derive(Clone, Debug, Default)]
pub struct Converter {
    opts: ConvertOptions,
}

impl Converter {
    /// A converter with default options.
    pub fn new() -> Self {
        Self {
            opts: ConvertOptions::default(),
        }
    }

    /// A converter with the supplied options.
    pub fn with_options(opts: ConvertOptions) -> Self {
        Self { opts }
    }

    /// Mutable access to the options.
    pub fn options_mut(&mut self) -> &mut ConvertOptions {
        &mut self.opts
    }

    /// Borrow the options.
    pub fn options(&self) -> &ConvertOptions {
        &self.opts
    }

    /// Convert Markdown to a [`Docx`].
    pub fn to_docx(&self, markdown: &str) -> Docx {
        engine::build_docx(markdown, &self.opts)
    }

    /// Convert Markdown to a `.docx` byte buffer.
    pub fn to_bytes(&self, markdown: &str) -> Result<Vec<u8>> {
        to_bytes_with(markdown, &self.opts)
    }

    /// Convert a Markdown string and write the `.docx` to `output`.
    pub fn write_file(&self, markdown: &str, output: impl AsRef<Path>) -> Result<()> {
        write_file_with(markdown, &self.opts, output)
    }

    /// Read a Markdown file and write the `.docx`. If `base_dir` is unset on the
    /// options, it is pointed at the input file's parent directory so that
    /// relative image references resolve.
    pub fn convert_file(&self, input: impl AsRef<Path>, output: impl AsRef<Path>) -> Result<()> {
        let input = input.as_ref();
        let md = std::fs::read_to_string(input)?;
        let mut opts = self.opts.clone();
        if opts.base_dir.is_none() {
            opts.base_dir = input.parent().map(|p| p.to_path_buf());
        }
        write_file_with(&md, &opts, output)
    }
}
