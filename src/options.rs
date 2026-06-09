//! Conversion options: page geometry, fonts, and which Markdown extensions to enable.

use std::path::PathBuf;

use pulldown_cmark::Options as CmarkOptions;

/// Page size + margins, all measured in twips (1/1440 inch).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageSetup {
    /// Page width in twips.
    pub width: u32,
    /// Page height in twips.
    pub height: u32,
    /// Uniform page margin in twips applied to all four sides.
    pub margin: u32,
}

impl PageSetup {
    /// US Letter, 8.5in x 11in, 1in margins.
    pub const LETTER: PageSetup = PageSetup {
        width: 12240,
        height: 15840,
        margin: 1440,
    };
    /// ISO A4, 210mm x 297mm, 1in margins.
    pub const A4: PageSetup = PageSetup {
        width: 11906,
        height: 16838,
        margin: 1440,
    };

    /// Width available for content (page width minus both margins), in twips.
    pub fn content_width(&self) -> u32 {
        self.width.saturating_sub(self.margin.saturating_mul(2))
    }

    /// Height available for content (page height minus both margins), in twips.
    pub fn content_height(&self) -> u32 {
        self.height.saturating_sub(self.margin.saturating_mul(2))
    }
}

impl Default for PageSetup {
    fn default() -> Self {
        PageSetup::LETTER
    }
}

/// Knobs controlling how Markdown is rendered into Word.
#[derive(Clone, Debug)]
pub struct ConvertOptions {
    /// Enable the GitHub Flavored Markdown bundle: tables, strikethrough,
    /// task lists, footnotes, and `[!NOTE]`-style alerts.
    pub gfm: bool,
    /// Turn straight quotes/dashes into typographic ones (`--` -> en dash, etc.).
    pub smart_punctuation: bool,
    /// Recognise `$inline$` and `$$display$$` math.
    pub math: bool,
    /// Render math as native Word equations (OMML / `m:oMath`) by translating a
    /// LaTeX subset, instead of as italic *Cambria Math* text. Requires [`math`].
    pub native_math: bool,
    /// Enable the `^superscript^` / `~subscript~` inline extensions (and map
    /// `<sup>` / `<sub>` HTML) to real Word vertical-alignment runs.
    pub super_sub: bool,
    /// Recognise `Term\n: definition` definition lists.
    pub definition_lists: bool,
    /// Honour `{#custom-id}` attributes on headings (used for anchor links).
    pub heading_attributes: bool,
    /// Parse a leading YAML (`---`) front-matter block (it is consumed, not rendered).
    pub yaml_front_matter: bool,
    /// Generate Word bookmarks on headings so intra-document `[x](#anchor)`
    /// links resolve.
    pub heading_anchors: bool,
    /// Render a Markdown soft line break (a single newline) as an actual line
    /// break instead of a space.
    pub soft_breaks_as_newlines: bool,
    /// Base directory used to resolve relative image paths. Defaults to the
    /// directory of the input file (CLI) or the process CWD (library).
    pub base_dir: Option<PathBuf>,
    /// Body font family (default `Calibri`).
    pub body_font: String,
    /// Heading font family (default `Calibri Light`).
    pub heading_font: String,
    /// Monospace font used for code (default `Consolas`).
    pub code_font: String,
    /// Body font size in points (default `11.0`).
    pub body_size_pt: f32,
    /// Code (monospace) font size in points, used by code blocks and inline
    /// `code` runs (default `10.0`).
    pub code_size_pt: f32,
    /// Caption / image-alt / definition-body font size in points (default `9.0`).
    pub caption_size_pt: f32,
    /// Heading font sizes in points, H1..H6 (default `[18, 16, 14, 13, 12, 11]`).
    pub heading_sizes_pt: [f32; 6],
    /// Heading accent colour as a 6-digit hex string, no `#` (default `000000`).
    pub heading_color: String,
    /// Hyperlink colour as a 6-digit hex string (default `0563C1`).
    pub link_color: String,
    /// Caption / muted-text colour as a 6-digit hex string (default `44546A`).
    pub caption_color: String,
    /// Block-quote text colour as a 6-digit hex string (default `404040`).
    pub quote_color: String,
    /// Left indent applied to block quotes, in twips (default `720`, i.e. 0.5in).
    pub quote_indent_twips: i32,
    /// Shading fill behind fenced/indented code blocks (default `F6F8FA`).
    pub code_fill: String,
    /// Shading fill behind inline `code` spans (default `EEF1F4`).
    pub inline_code_fill: String,
    /// Shading fill behind table header cells (default `F2F2F2`).
    pub header_fill: String,
    /// Page geometry.
    pub page: PageSetup,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        ConvertOptions {
            gfm: true,
            smart_punctuation: false,
            math: true,
            native_math: true,
            super_sub: true,
            definition_lists: true,
            heading_attributes: true,
            yaml_front_matter: true,
            heading_anchors: true,
            soft_breaks_as_newlines: false,
            base_dir: None,
            body_font: "Calibri".to_string(),
            heading_font: "Calibri Light".to_string(),
            code_font: "Consolas".to_string(),
            body_size_pt: 11.0,
            code_size_pt: 10.0,
            caption_size_pt: 9.0,
            heading_sizes_pt: [18.0, 16.0, 14.0, 13.0, 12.0, 11.0],
            heading_color: "000000".to_string(),
            link_color: "0563C1".to_string(),
            caption_color: "44546A".to_string(),
            quote_color: "404040".to_string(),
            quote_indent_twips: 720,
            code_fill: "F6F8FA".to_string(),
            inline_code_fill: "EEF1F4".to_string(),
            header_fill: "F2F2F2".to_string(),
            page: PageSetup::default(),
        }
    }
}

impl ConvertOptions {
    /// Body size expressed in half-points, the unit `docx-rs` uses for run sizes.
    pub(crate) fn body_half_points(&self) -> usize {
        (self.body_size_pt * 2.0).round() as usize
    }

    /// Code (monospace) size in half-points.
    pub(crate) fn code_half_points(&self) -> usize {
        (self.code_size_pt * 2.0).round() as usize
    }

    /// Caption size in half-points.
    pub(crate) fn caption_half_points(&self) -> usize {
        (self.caption_size_pt * 2.0).round() as usize
    }

    /// Size of heading level `level` (1..=6) in half-points. Levels outside the
    /// range clamp to the nearest valid heading.
    pub(crate) fn heading_half_points(&self, level: usize) -> usize {
        let i = level.clamp(1, 6) - 1;
        (self.heading_sizes_pt[i] * 2.0).round() as usize
    }

    /// Translate these options into the `pulldown-cmark` parser flags.
    pub(crate) fn cmark_options(&self) -> CmarkOptions {
        let mut o = CmarkOptions::empty();
        if self.gfm {
            o.insert(CmarkOptions::ENABLE_TABLES);
            o.insert(CmarkOptions::ENABLE_STRIKETHROUGH);
            o.insert(CmarkOptions::ENABLE_TASKLISTS);
            o.insert(CmarkOptions::ENABLE_FOOTNOTES);
            // ENABLE_GFM only gates [!NOTE]/[!WARNING] alert block quotes.
            o.insert(CmarkOptions::ENABLE_GFM);
        }
        if self.smart_punctuation {
            o.insert(CmarkOptions::ENABLE_SMART_PUNCTUATION);
        }
        if self.math {
            o.insert(CmarkOptions::ENABLE_MATH);
        }
        if self.super_sub {
            o.insert(CmarkOptions::ENABLE_SUPERSCRIPT);
            o.insert(CmarkOptions::ENABLE_SUBSCRIPT);
        }
        if self.definition_lists {
            o.insert(CmarkOptions::ENABLE_DEFINITION_LIST);
        }
        if self.heading_attributes {
            o.insert(CmarkOptions::ENABLE_HEADING_ATTRIBUTES);
        }
        if self.yaml_front_matter {
            o.insert(CmarkOptions::ENABLE_YAML_STYLE_METADATA_BLOCKS);
        }
        o
    }
}
