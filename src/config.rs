//! A serde-deserialisable [`Theme`] describing every overridable styling knob,
//! loadable from a TOML file and applied onto a [`ConvertOptions`].
//!
//! Every field is optional: an absent key leaves the corresponding option at its
//! current value, so a theme file only needs to mention what it wants to change.
//! Colours are 6-digit hex strings (a leading `#` is accepted and stripped);
//! `heading_sizes`, when present, must list exactly six point sizes (H1..H6).
//!
//! ```toml
//! # theme.toml
//! body_font     = "Georgia"
//! heading_font  = "Georgia"
//! code_font     = "Cascadia Code"
//! body_size     = 11.5
//! heading_sizes = [22, 18, 15, 13, 12, 11]
//! heading_color = "#1F3864"
//! link_color    = "0563C1"
//! code_fill     = "F6F8FA"
//! page          = "a4"
//! smart         = true
//! ```

use std::path::Path;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::options::{ConvertOptions, PageSetup};

/// A bundle of optional style/rendering overrides, typically read from a TOML
/// theme file. Apply it onto a [`ConvertOptions`] with [`Theme::apply`].
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Theme {
    // -- Fonts -------------------------------------------------------------
    /// Body font family.
    pub body_font: Option<String>,
    /// Heading font family.
    pub heading_font: Option<String>,
    /// Monospace font family for code.
    pub code_font: Option<String>,

    // -- Sizes (points) ----------------------------------------------------
    /// Body font size.
    pub body_size: Option<f32>,
    /// Code (monospace) font size.
    pub code_size: Option<f32>,
    /// Caption / muted-text font size.
    pub caption_size: Option<f32>,
    /// Exactly six heading sizes, H1..H6.
    pub heading_sizes: Option<Vec<f32>>,

    // -- Colours (6-digit hex, `#` optional) -------------------------------
    /// Heading accent colour.
    pub heading_color: Option<String>,
    /// Hyperlink colour.
    pub link_color: Option<String>,
    /// Caption / muted-text colour.
    pub caption_color: Option<String>,
    /// Block-quote text colour.
    pub quote_color: Option<String>,
    /// Fenced/indented code-block shading fill.
    pub code_fill: Option<String>,
    /// Inline `code` shading fill.
    pub inline_code_fill: Option<String>,
    /// Table header-cell shading fill.
    pub header_fill: Option<String>,

    // -- Block geometry ----------------------------------------------------
    /// Block-quote left indent, in twips.
    pub quote_indent: Option<i32>,
    /// Page size: `"letter"` or `"a4"`.
    pub page: Option<String>,

    // -- Rendering toggles -------------------------------------------------
    /// Enable the GitHub Flavored Markdown bundle.
    pub gfm: Option<bool>,
    /// Smart (typographic) punctuation.
    pub smart: Option<bool>,
    /// `$math$` parsing.
    pub math: Option<bool>,
    /// Native Word equations (OMML) for math.
    pub native_math: Option<bool>,
    /// `^sup^` / `~sub~` extensions.
    pub super_sub: Option<bool>,
    /// `Term\n: definition` definition lists.
    pub definition_lists: Option<bool>,
    /// `{#id}` heading attributes.
    pub heading_attributes: Option<bool>,
    /// Leading YAML front-matter block.
    pub yaml_front_matter: Option<bool>,
    /// Heading bookmarks for intra-document anchors.
    pub heading_anchors: Option<bool>,
    /// Render single newlines as hard line breaks.
    pub soft_breaks: Option<bool>,
}

impl Theme {
    /// Parse a [`Theme`] from a TOML string.
    pub fn from_toml_str(s: &str) -> Result<Self> {
        toml::from_str(s).map_err(|e| Error::Config(e.to_string()))
    }

    /// Read and parse a [`Theme`] from a TOML file on disk.
    pub fn from_toml_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|e| {
            Error::Config(format!("could not read theme file {}: {e}", path.display()))
        })?;
        Self::from_toml_str(&text)
    }

    /// Overlay every set field onto `opts`, validating colours, heading-size
    /// arity, and the page name. Fields left as `None` are untouched.
    pub fn apply(&self, opts: &mut ConvertOptions) -> Result<()> {
        if let Some(v) = &self.body_font {
            opts.body_font = v.clone();
        }
        if let Some(v) = &self.heading_font {
            opts.heading_font = v.clone();
        }
        if let Some(v) = &self.code_font {
            opts.code_font = v.clone();
        }

        if let Some(v) = self.body_size {
            opts.body_size_pt = v;
        }
        if let Some(v) = self.code_size {
            opts.code_size_pt = v;
        }
        if let Some(v) = self.caption_size {
            opts.caption_size_pt = v;
        }
        if let Some(v) = &self.heading_sizes {
            if v.len() != 6 {
                return Err(Error::Config(format!(
                    "heading_sizes must list exactly 6 sizes (H1..H6), got {}",
                    v.len()
                )));
            }
            opts.heading_sizes_pt = [v[0], v[1], v[2], v[3], v[4], v[5]];
        }

        if let Some(v) = &self.heading_color {
            opts.heading_color = normalize_hex("heading_color", v)?;
        }
        if let Some(v) = &self.link_color {
            opts.link_color = normalize_hex("link_color", v)?;
        }
        if let Some(v) = &self.caption_color {
            opts.caption_color = normalize_hex("caption_color", v)?;
        }
        if let Some(v) = &self.quote_color {
            opts.quote_color = normalize_hex("quote_color", v)?;
        }
        if let Some(v) = &self.code_fill {
            opts.code_fill = normalize_hex("code_fill", v)?;
        }
        if let Some(v) = &self.inline_code_fill {
            opts.inline_code_fill = normalize_hex("inline_code_fill", v)?;
        }
        if let Some(v) = &self.header_fill {
            opts.header_fill = normalize_hex("header_fill", v)?;
        }

        if let Some(v) = self.quote_indent {
            opts.quote_indent_twips = v;
        }
        if let Some(v) = &self.page {
            opts.page = parse_page(v)?;
        }

        if let Some(v) = self.gfm {
            opts.gfm = v;
        }
        if let Some(v) = self.smart {
            opts.smart_punctuation = v;
        }
        if let Some(v) = self.math {
            opts.math = v;
        }
        if let Some(v) = self.native_math {
            opts.native_math = v;
        }
        if let Some(v) = self.super_sub {
            opts.super_sub = v;
        }
        if let Some(v) = self.definition_lists {
            opts.definition_lists = v;
        }
        if let Some(v) = self.heading_attributes {
            opts.heading_attributes = v;
        }
        if let Some(v) = self.yaml_front_matter {
            opts.yaml_front_matter = v;
        }
        if let Some(v) = self.heading_anchors {
            opts.heading_anchors = v;
        }
        if let Some(v) = self.soft_breaks {
            opts.soft_breaks_as_newlines = v;
        }

        Ok(())
    }
}

/// Validate and canonicalise a hex colour: strip an optional leading `#`, require
/// exactly six hex digits, and upper-case the result. `field` names the source
/// key for a helpful error.
pub fn normalize_hex(field: &str, value: &str) -> Result<String> {
    let s = value.trim().strip_prefix('#').unwrap_or(value.trim());
    if s.len() == 6 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(s.to_ascii_uppercase())
    } else {
        Err(Error::Config(format!(
            "{field}: invalid hex colour {value:?} (expected 6 hex digits, e.g. \"1A2B3C\")"
        )))
    }
}

/// Map a case-insensitive page name (`letter` / `a4`) to a [`PageSetup`].
pub fn parse_page(name: &str) -> Result<PageSetup> {
    match name.trim().to_ascii_lowercase().as_str() {
        "letter" => Ok(PageSetup::LETTER),
        "a4" => Ok(PageSetup::A4),
        other => Err(Error::Config(format!(
            "unknown page size {other:?} (expected \"letter\" or \"a4\")"
        ))),
    }
}
