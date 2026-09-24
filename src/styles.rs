//! Word style definitions and document-level setup (page geometry, default fonts,
//! and the paragraph/character styles the engine references by id).

use docx_rs::*;

use crate::options::ConvertOptions;

// ---- Style ids referenced by the engine ------------------------------------

/// Heading style ids, indexed by `level - 1` (so `HEADING[0]` == "Heading1").
pub(crate) const HEADING: [&str; 6] = [
    "Heading1", "Heading2", "Heading3", "Heading4", "Heading5", "Heading6",
];
/// Paragraph style for block quotes.
pub(crate) const QUOTE: &str = "Quote";
/// Paragraph style for fenced/indented code blocks.
pub(crate) const SOURCE_CODE: &str = "SourceCode";
/// Character style for inline `code` spans.
pub(crate) const VERBATIM_CHAR: &str = "VerbatimChar";
/// Character style for hyperlinks.
pub(crate) const HYPERLINK: &str = "Hyperlink";
/// Paragraph style for image captions / definition-list definitions.
pub(crate) const CAPTION: &str = "Caption";

/// Heading sizes in half-points (H1..H6): 18, 16, 14, 13, 12, 11 pt.
const HEADING_HALF_PT: [usize; 6] = [36, 32, 28, 26, 24, 22];
/// Word's default heading accent colour.
const HEADING_COLOR: &str = "000000";
/// Shading fill behind code blocks (GitHub-ish light grey).
pub(crate) const CODE_FILL: &str = "F6F8FA";
/// Shading fill behind inline code spans (kept coherent with [`CODE_FILL`]).
pub(crate) const INLINE_CODE_FILL: &str = "EEF1F4";
/// Shading fill behind table header cells.
pub(crate) const HEADER_FILL: &str = "F2F2F2";
/// Colour used for image-alt captions / muted text.
pub(crate) const CAPTION_COLOR: &str = "44546A";
/// Hyperlink colour.
const LINK_COLOR: &str = "0563C1";

/// Apply page geometry, document defaults, and register every style the engine
/// uses. Returns the configured (still empty) document.
/// `lang` is the document language, already resolved from the options and the
/// front matter (see [`crate::engine::document_lang`]).
pub(crate) fn setup(mut docx: Docx, opts: &ConvertOptions, lang: &str) -> Docx {
    let m = opts.page.margin as i32;
    let margin = PageMargin {
        top: m,
        left: m,
        bottom: m,
        right: m,
        header: 720,
        footer: 720,
        gutter: 0,
    };

    docx = docx
        .page_size(opts.page.width, opts.page.height)
        .page_margin(margin)
        .default_fonts(
            RunFonts::new()
                .ascii(opts.body_font.clone())
                .hi_ansi(opts.body_font.clone())
                .east_asia(opts.body_font.clone()),
        )
        .default_size(opts.body_half_points())
        // Latin-script text: tag the language, and leave out the East Asian
        // compatibility flags upstream docx-rs writes by default. With
        // `balanceSingleByteDoubleByteWidth` LibreOffice measures "≈" or "ẹ" at
        // East Asian widths and justified lines overrun the right margin.
        .default_lang(Lang::new(lang))
        .east_asian_compat(false)
        // ~1.08 line height with 8pt after each paragraph, matching Word's
        // modern default so the body does not render cramped.
        .default_line_spacing(
            LineSpacing::new()
                .line_rule(LineSpacingType::Auto)
                .line(259)
                .after(160),
        );

    // Headings.
    for (i, id) in HEADING.iter().enumerate() {
        let mut s = Style::new(*id, StyleType::Paragraph)
            .name(format!("heading {}", i + 1))
            .based_on("Normal")
            .next("Normal")
            .outline_lvl(i)
            .size(HEADING_HALF_PT[i])
            .color(HEADING_COLOR)
            .bold()
            .line_spacing(
                LineSpacing::new()
                    .line_rule(LineSpacingType::Auto)
                    .line(259)
                    .before(if i == 0 { 240 } else { 200 })
                    .after(80),
            )
            .fonts(
                RunFonts::new()
                    .ascii(opts.heading_font.clone())
                    .hi_ansi(opts.heading_font.clone()),
            );
        if i >= 5 {
            s = s.italic();
        }
        docx = docx.add_style(s);
    }

    // Block quote.
    docx = docx.add_style(
        Style::new(QUOTE, StyleType::Paragraph)
            .name("Quote")
            .based_on("Normal")
            .italic()
            .color("404040")
            .indent(Some(720), None, None, None),
    );

    // Code block paragraph (runs also set the monospace font explicitly so the
    // look survives even if the style is stripped).
    docx = docx.add_style(
        Style::new(SOURCE_CODE, StyleType::Paragraph)
            .name("Source Code")
            .based_on("Normal")
            .fonts(
                RunFonts::new()
                    .ascii(opts.code_font.clone())
                    .hi_ansi(opts.code_font.clone()),
            )
            .size(20),
    );

    // Caption (image alt fallback, definition bodies).
    docx = docx.add_style(
        Style::new(CAPTION, StyleType::Paragraph)
            .name("Caption")
            .based_on("Normal")
            .italic()
            .color(CAPTION_COLOR)
            .size(18),
    );

    // Inline code character style.
    docx = docx.add_style(
        Style::new(VERBATIM_CHAR, StyleType::Character)
            .name("Verbatim Char")
            .fonts(
                RunFonts::new()
                    .ascii(opts.code_font.clone())
                    .hi_ansi(opts.code_font.clone()),
            )
            .size(20),
    );

    // Hyperlink character style.
    docx = docx.add_style(
        Style::new(HYPERLINK, StyleType::Character)
            .name("Hyperlink")
            .color(LINK_COLOR)
            .underline("single"),
    );

    docx
}
