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

// Note: heading sizes, accent/link/caption/quote colours, and the code/inline/
// header shading fills are no longer hardcoded here — they live on
// [`ConvertOptions`] so they can be overridden via the CLI or a theme file.

/// Apply page geometry, document defaults, and register every style the engine
/// uses. Returns the configured (still empty) document.
pub(crate) fn setup(mut docx: Docx, opts: &ConvertOptions) -> Docx {
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
            .size(opts.heading_half_points(i + 1))
            .color(opts.heading_color.clone())
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
            .color(opts.quote_color.clone())
            .indent(Some(opts.quote_indent_twips), None, None, None),
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
            .size(opts.code_half_points()),
    );

    // Caption (image alt fallback, definition bodies).
    docx = docx.add_style(
        Style::new(CAPTION, StyleType::Paragraph)
            .name("Caption")
            .based_on("Normal")
            .italic()
            .color(opts.caption_color.clone())
            .size(opts.caption_half_points()),
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
            .size(opts.code_half_points()),
    );

    // Hyperlink character style.
    docx = docx.add_style(
        Style::new(HYPERLINK, StyleType::Character)
            .name("Hyperlink")
            .color(opts.link_color.clone())
            .underline("single"),
    );

    docx
}
