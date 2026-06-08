//! Integration tests: convert Markdown to a `.docx` byte buffer, unzip it, and
//! assert on the generated `word/*.xml` parts.

use std::io::{Cursor, Read};

use rust_knit_md_docx::{ConvertOptions, Converter, to_bytes, to_bytes_with};

/// Convert Markdown and return the contents of a named part inside the `.docx`.
fn part(markdown: &str, name: &str) -> String {
    let bytes = to_bytes(markdown).expect("conversion should succeed");
    read_part(&bytes, name)
}

fn read_part(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).expect("valid zip");
    let mut f = zip
        .by_name(name)
        .unwrap_or_else(|_| panic!("missing part {name}"));
    let mut s = String::new();
    f.read_to_string(&mut s).expect("utf-8 part");
    s
}

fn document(markdown: &str) -> String {
    part(markdown, "word/document.xml")
}

#[test]
fn output_is_a_zip() {
    let bytes = to_bytes("# Hi").unwrap();
    assert_eq!(&bytes[..2], b"PK", "docx must be a zip archive");
}

#[test]
fn empty_input_does_not_panic() {
    let bytes = to_bytes("").unwrap();
    assert_eq!(&bytes[..2], b"PK");
}

#[test]
fn headings_get_heading_styles() {
    let doc = document("# H1\n\n## H2\n\n### H3\n\n#### H4\n\n##### H5\n\n###### H6");
    for n in 1..=6 {
        assert!(
            doc.contains(&format!("w:val=\"Heading{n}\"")),
            "expected Heading{n} style"
        );
    }
}

#[test]
fn inline_formatting_renders() {
    let doc = document("**bold** *italic* ~~struck~~ `code`");
    assert!(doc.contains("<w:b />") || doc.contains("<w:b/>"), "bold");
    assert!(doc.contains("<w:i />") || doc.contains("<w:i/>"), "italic");
    assert!(doc.contains("<w:strike"), "strikethrough");
    assert!(doc.contains("w:val=\"VerbatimChar\""), "inline code style");
}

#[test]
fn external_link_renders_as_hyperlink() {
    let doc = document("[example](https://example.com)");
    assert!(doc.contains("<w:hyperlink"), "hyperlink element");
    assert!(doc.contains("w:val=\"Hyperlink\""), "hyperlink run style");
}

#[test]
fn internal_anchor_link_matches_a_heading_bookmark() {
    let doc = document("# My Section\n\nSee [it](#my-section).");
    assert!(
        doc.contains("w:name=\"my-section\""),
        "heading should emit a bookmark named after its slug"
    );
    assert!(
        doc.contains("w:anchor=\"my-section\""),
        "internal link should target the slug anchor"
    );
}

#[test]
fn unordered_and_ordered_lists_emit_numbering() {
    let doc = document("- a\n- b\n\n1. one\n2. two");
    let num = part("- a\n- b\n\n1. one\n2. two", "word/numbering.xml");
    assert!(doc.contains("<w:numPr>"), "list paragraphs carry numPr");
    assert!(
        num.contains("w:val=\"bullet\""),
        "a bullet numbering exists"
    );
    assert!(
        num.contains("w:val=\"decimal\""),
        "a decimal numbering exists"
    );
}

#[test]
fn ordered_list_honors_custom_start() {
    let num = part("5. five\n6. six", "word/numbering.xml");
    assert!(
        num.contains("<w:start w:val=\"5\"") || num.contains("w:val=\"5\""),
        "custom start value 5 should appear in numbering.xml"
    );
}

#[test]
fn nested_lists_allocate_distinct_numberings() {
    let num = part("1. a\n   1. b\n   2. c\n2. d", "word/numbering.xml");
    // Each list (outer + nested) gets its own abstractNum so they restart and
    // indent independently.
    let abstract_count = num.matches("<w:abstractNum ").count();
    assert!(
        abstract_count >= 2,
        "expected at least two abstract numberings, got {abstract_count}"
    );
}

#[test]
fn task_list_renders_checkboxes() {
    let doc = document("- [x] done\n- [ ] todo");
    assert!(doc.contains('\u{2612}'), "checked box glyph");
    assert!(doc.contains('\u{2610}'), "unchecked box glyph");
}

#[test]
fn code_block_preserves_indentation_in_a_shaded_table() {
    let doc = document("```rust\nfn main() {\n    let x = 1;\n}\n```");
    assert!(doc.contains("<w:tbl>"), "code block wrapped in a table");
    assert!(doc.contains("w:val=\"SourceCode\""), "SourceCode style");
    assert!(
        doc.contains("xml:space=\"preserve\">    let x"),
        "leading indentation preserved"
    );
    assert!(doc.contains("<w:shd "), "code block has shading");
}

#[test]
fn block_quote_uses_quote_style() {
    let doc = document("> quoted text");
    assert!(doc.contains("w:val=\"Quote\""), "Quote paragraph style");
}

#[test]
fn gfm_alert_emits_a_label() {
    let doc = document("> [!WARNING]\n> careful");
    assert!(doc.contains("Warning"), "alert label text present");
}

#[test]
fn table_renders_with_header_and_alignment() {
    let md = "| L | C | R |\n|:--|:-:|--:|\n| a | b | c |";
    let doc = document(md);
    assert!(doc.contains("<w:tbl>"), "table element");
    assert!(doc.contains("w:val=\"center\""), "center-aligned column");
    assert!(doc.contains("w:val=\"right\""), "right-aligned column");
    assert!(doc.contains("<w:shd "), "header cell shading");
}

#[test]
fn footnotes_resolve_to_real_word_notes() {
    let md = "Text.[^a]\n\n[^a]: the note body.";
    let bytes = to_bytes(md).unwrap();
    let doc = read_part(&bytes, "word/document.xml");
    let notes = read_part(&bytes, "word/footnotes.xml");
    assert!(doc.contains("<w:footnoteReference"), "reference in body");
    assert!(notes.contains("the note body"), "note body present");
}

#[test]
fn thematic_break_renders_centered_rule() {
    let doc = document("a\n\n---\n\nb");
    assert!(doc.contains("w:val=\"center\""), "rule is centered");
    assert!(doc.contains('\u{2014}'), "rule uses em dashes");
}

#[test]
fn smart_punctuation_is_opt_in() {
    let plain = document("\"quoted\" and -- dashes");
    assert!(
        plain.contains("&quot;quoted&quot;") || plain.contains("\"quoted\""),
        "straight quotes preserved by default"
    );

    let opts = ConvertOptions {
        smart_punctuation: true,
        ..ConvertOptions::default()
    };
    let bytes = to_bytes_with("\"quoted\"", &opts).unwrap();
    let smart = read_part(&bytes, "word/document.xml");
    assert!(
        smart.contains('\u{201c}') || smart.contains('\u{201d}'),
        "curly quotes when smart punctuation is enabled"
    );
}

#[test]
fn converter_with_a4_page_size() {
    let opts = ConvertOptions {
        page: rust_knit_md_docx::PageSetup::A4,
        ..ConvertOptions::default()
    };
    let bytes = Converter::with_options(opts).to_bytes("# A4").unwrap();
    let doc = read_part(&bytes, "word/document.xml");
    // A4 width in twips.
    assert!(doc.contains("11906"), "A4 page width should appear");
}

#[test]
fn yaml_front_matter_is_not_rendered() {
    let doc = document("---\ntitle: Secret\n---\n\n# Visible");
    assert!(doc.contains("Visible"), "body heading present");
    assert!(
        !doc.contains("Secret"),
        "front matter must not leak into body"
    );
}

#[test]
fn data_uri_image_is_embedded() {
    // A valid 1x1 PNG. Exercises base64 decode -> validate -> re-encode PNG ->
    // Pic::new_with_dimensions (the panic-free image path).
    let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
    let md = format!("![dot](data:image/png;base64,{png})\n\nAfter.");
    let bytes = to_bytes(&md).unwrap();

    let mut zip = zip::ZipArchive::new(Cursor::new(bytes.clone())).unwrap();
    let media: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_string())
        .filter(|n| n.starts_with("word/media/"))
        .collect();
    assert!(!media.is_empty(), "an image part should be embedded");

    let doc = read_part(&bytes, "word/document.xml");
    assert!(doc.contains("<w:drawing"), "document references a drawing");
    assert!(doc.contains("After"), "trailing text preserved");
}

#[test]
fn bad_image_falls_back_to_caption_without_panicking() {
    // Undecodable bytes must never panic; we get an alt-text caption instead.
    let md = "![broken](data:image/png;base64,bm90YW5pbWFnZQ==)";
    let doc = document(md);
    assert!(
        doc.contains("broken") || doc.contains("image:"),
        "alt-text fallback"
    );
    assert!(
        !doc.contains("<w:drawing"),
        "no image embedded for bad bytes"
    );
}

#[test]
fn lists_never_use_reserved_numid_1() {
    // docx-rs reserves abstractNumId=1 / numId=1 for a built-in default; our
    // lists must start at 2 to avoid a duplicate-id clash that makes Word render
    // the first list with the wrong format.
    let bytes = to_bytes("- a\n- b").unwrap();
    let doc = read_part(&bytes, "word/document.xml");
    assert!(
        !doc.contains("<w:numId w:val=\"1\""),
        "lists must not reference the reserved numId=1"
    );
    assert!(
        doc.contains("<w:numId w:val=\"2\""),
        "first list should use numId 2"
    );
}

#[test]
fn bare_urls_are_autolinked() {
    let doc = document("See https://example.com for details.");
    assert!(doc.contains("<w:hyperlink"), "bare URL becomes a hyperlink");
    // Surrounding text is preserved as ordinary runs.
    assert!(doc.contains("for details"), "trailing text preserved");
}

#[test]
fn code_block_table_has_no_black_borders() {
    let doc = document("```\nx\n```");
    assert!(doc.contains("<w:tbl>"), "code wrapped in a table");
    // `Table::without_borders` emits explicit `nil`/`none` borders rather than
    // the default single black grid.
    assert!(
        !doc.contains("w:val=\"single\""),
        "code block table should not have single black borders"
    );
}

#[test]
fn footnote_with_a_link_does_not_emit_a_dangling_relationship() {
    // A hyperlink inside a footnote body would allocate an r:id that
    // footnotes.xml.rels never registers -> Word repair. We flatten it instead.
    let md = "Text.[^a]\n\n[^a]: see [the site](https://example.com).";
    let bytes = to_bytes(md).unwrap();
    let notes = read_part(&bytes, "word/footnotes.xml");
    assert!(
        !notes.contains("<w:hyperlink"),
        "footnote links are flattened"
    );
    assert!(
        notes.contains("example.com"),
        "the URL is still shown as text"
    );
}

#[test]
fn document_has_default_line_spacing() {
    let doc = document("Hello world.");
    assert!(
        read_part(&to_bytes("x").unwrap(), "word/styles.xml").contains("<w:spacing"),
        "document defaults should set paragraph spacing"
    );
    let _ = doc;
}

#[test]
fn all_numbering_references_resolve() {
    let md = "- a\n  - b\n- c\n\n1. x\n2. y\n\n- [ ] t";
    let bytes = to_bytes(md).unwrap();
    let doc = read_part(&bytes, "word/document.xml");
    let num = read_part(&bytes, "word/numbering.xml");

    let used: std::collections::HashSet<&str> = doc
        .match_indices("<w:numId w:val=\"")
        .map(|(i, m)| {
            let rest = &doc[i + m.len()..];
            &rest[..rest.find('"').unwrap()]
        })
        .collect();
    for id in used {
        assert!(
            num.contains(&format!("<w:num w:numId=\"{id}\"")),
            "numId {id} referenced in body must be defined in numbering.xml"
        );
    }
}
