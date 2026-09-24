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

/// The language a document is tagged with when neither the options nor the
/// front matter name one.
pub const DEFAULT_LANG: &str = "en-US";

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
    /// Page geometry.
    pub page: PageSetup,
    /// Document language, a BCP 47 tag such as `en-US`, `de-DE` or `it-IT`,
    /// written as the document-default `w:lang`. Word and LibreOffice pick
    /// spelling, hyphenation and line-breaking rules by it. `None` takes the
    /// YAML front matter's `lang:` when there is one, else [`DEFAULT_LANG`].
    pub lang: Option<String>,
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
            page: PageSetup::default(),
            lang: None,
        }
    }
}

impl ConvertOptions {
    /// Body size expressed in half-points, the unit `docx-rs` uses for run sizes.
    pub(crate) fn body_half_points(&self) -> usize {
        (self.body_size_pt * 2.0).round() as usize
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
