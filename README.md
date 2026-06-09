# rust_knit_md_docx

> Knit Markdown into a Microsoft Word `.docx` — with high fidelity.

A Rust library **and** command-line tool that converts Markdown
([CommonMark](https://commonmark.org/) + [GitHub Flavored Markdown](https://github.github.com/gfm/))
into a real Word document. It pairs [`pulldown-cmark`](https://crates.io/crates/pulldown-cmark)
(a fast, spec-compliant parser) with [`docx-rs`](https://crates.io/crates/docx-rs)
(an OOXML writer), and folds the parser's event stream into properly styled
Word paragraphs, runs, tables, lists, and footnotes.

The output opens cleanly in Microsoft Word, LibreOffice Writer, and Google Docs
— no "this document needs to be repaired" prompts.

## Features

| Markdown | Rendered as |
| --- | --- |
| `#` … `######` headings | Word `Heading 1`–`Heading 6` styles (with outline levels, so the navigation pane and TOC work) |
| `**bold**`, `*italic*`, `***both***` | Bold / italic runs, correctly nested |
| `~~strikethrough~~` | Struck-through runs |
| `` `inline code` `` | Monospace `Verbatim Char` runs with light shading |
| `[text](url)` | External hyperlinks (blue, underlined) |
| `[text](#heading)` | Internal anchor links that jump to the heading (via bookmarks) |
| `<https://…>`, `<a@b.com>` | Autolinks and `mailto:` email autolinks |
| bare `https://…` in text | Linkified automatically (GFM extended autolinking) |
| `![alt](path.png)` | Embedded, auto-sized images (scaled to fit the page width) |
| `- item` / `1. item` | Bullet and numbered lists, with restart and custom start values |
| nested lists | Independent numbering per level, mixed bullet/number nesting |
| `- [x] task` | Task lists with ☒ / ☐ checkboxes |
| ```` ```lang ```` fenced code | A shaded, full-width code box with indentation preserved |
| `> quote` | `Quote` paragraph style; nested quotes indent further |
| `> [!NOTE]` GFM alerts | A coloured **Note / Tip / Important / Warning / Caution** label |
| GFM tables | Bordered tables with a shaded header row and per-column alignment |
| `---` thematic break | A horizontal rule (an empty paragraph with a bottom border) |
| `text[^1]` + `[^1]: …` | **Real Word footnotes** (auto-numbered by Word) |
| definition lists | Bold term + indented definition |
| `$math$`, `$$math$$` | **Native Word equations** (OMML / `m:oMath`) via a LaTeX-subset translator |
| `^sup^`, `~sub~`, `<sup>`, `<sub>` | Real superscript / subscript runs (`w:vertAlign`) |
| inline HTML (`<b>`, `<br>`, `<mark>`, …) | A best-effort subset mapped to runs |
| YAML front matter | Parsed and **not** rendered into the body |

## Install

### As a command-line tool

```sh
cargo install --path .
# or, from the repo root:
cargo build --release   # binary at target/release/knit-md-docx
```

### As a library

```toml
[dependencies]
rust_knit_md_docx = { git = "https://github.com/danganhtu01/rust_knit_md_docx" }
```

> This crate depends on a [fork of `docx-rs`](https://github.com/danganhtu01/docx-rs)
> (for native equations, vertical-alignment runs, and paragraph borders), wired up
> as a `path` dependency in [`Cargo.toml`](Cargo.toml). Point it at your checkout of
> the fork, or switch it to a `git` dependency.

## Command-line usage

```sh
# Simplest: writes sample.docx next to the input
knit-md-docx sample.md

# Choose the output path
knit-md-docx notes.md -o report.docx

# Read from stdin
cat notes.md | knit-md-docx - -o out.docx

# Options
knit-md-docx in.md \
  --page a4 \                # letter (default) | a4
  --smart \                  # typographic punctuation
  --soft-breaks \            # single newlines become line breaks
  --no-gfm \                 # disable tables/tasklists/footnotes/strikethrough/alerts
  --no-anchors \             # don't emit heading bookmarks
  --body-font "Georgia" \
  --code-font "Cascadia Code" \
  --body-size 12
```

Run `knit-md-docx --help` for the full list.

## Library usage

```rust
use rust_knit_md_docx::{convert_file, to_bytes, Converter, ConvertOptions, PageSetup};

// 1. File in, file out (relative image paths resolve next to the input).
convert_file("README.md", "README.docx")?;

// 2. String in, bytes out (e.g. to stream over HTTP).
let bytes: Vec<u8> = to_bytes("# Hello\n\nWorld.")?;

// 3. Full control over options, and access to the raw `docx_rs::Docx`.
let mut opts = ConvertOptions::default();
opts.page = PageSetup::A4;
opts.smart_punctuation = true;
opts.code_font = "Cascadia Code".to_string();

let converter = Converter::with_options(opts);
converter.write_file("# Title\n\nBody.", "out.docx")?;

// Get a `Docx` you can keep customising before packing it yourself:
let docx = converter.to_docx("# Title");
docx.build().pack(std::fs::File::create("custom.docx")?)?;
# Ok::<(), rust_knit_md_docx::Error>(())
```

## How it works

Word's document model is **flat** — the body is a list of paragraphs and tables,
and runs cannot nest — whereas Markdown is a tree. The engine
([`src/engine.rs`](src/engine.rs)) bridges the two by folding `pulldown-cmark`'s
balanced `Start`/`End` event stream while maintaining a few explicit stacks:

- an **inline-formatting** stack (bold/italic/strike/underline/code as counters,
  because the same emphasis can nest),
- a **list** stack (each list gets a freshly registered numbering so ordered
  lists restart and nested lists indent independently),
- a **block-quote** depth, an open **link**/**image**, and a **table** builder.

Every leaf inline event materialises *one fully-resolved run* stamped with the
formatting that is live at that moment, which is then routed to the open link,
the open table cell, or the current paragraph. Footnotes are collected in a
pre-pass so each reference can carry its body (Word stores footnote content at
the reference site).

Units, for reference: page geometry and indents are in **twips** (1/1440″), run
sizes in **half-points**, and image dimensions in **EMU** (1px = 9525 EMU).

## Native equations, superscript & rules — a forked `docx-rs`

Three features need OOXML surface the published `docx-rs 0.4.20` does not expose,
so this crate depends on a small [**fork**](https://github.com/danganhtu01/docx-rs)
(wired up via a `path`/`git` dependency) that adds:

- **OMML equations** — an `OMath` / `OMathElement` tree that emits `m:oMath`
  (runs, super/subscripts, fractions, radicals, n-ary operators, delimiters,
  functions) plus the `xmlns:m` namespace on `w:document`.
- **`Run::superscript()` / `Run::subscript()`** — run-level `w:vertAlign`.
- **`Paragraph::set_borders()`** — paragraph borders (used for the rule).

[`src/math.rs`](src/math.rs) translates a useful subset of LaTeX math into that
`OMath` tree, so `$x^2$` and `$$\frac{a}{b}$$` become **real, editable Word
equations** rather than styled text. Unrecognised LaTeX degrades to literal text,
so nothing is ever lost.

## Known limitations

These are rendered as documented fallbacks rather than failing:

- **Math** translates a *subset* of LaTeX (super/subscripts, `\frac`, `\sqrt`,
  `\sum`/`\int` with limits, `\left(…\right)`, Greek + a broad symbol table).
  Unsupported constructs (matrices, `\begin{…}` environments, accents like
  `\hat`) fall back to literal text inside the equation. Math inside a hyperlink
  is rendered as text (an `m:oMath` cannot be a hyperlink child). Deeply nested
  input is depth-capped (the remainder becomes literal text) so adversarial
  Markdown cannot overflow the stack. Set `native_math: false` to revert to the
  old italic *Cambria Math* text.
- **Equations are write-only.** The fork emits `m:oMath`/`m:oMathPara`, but the
  `docx-rs` reader does not parse them, so reading a `.docx` and writing it back
  out with `docx-rs` drops any equations. This converter only writes, so it is
  unaffected; it matters only if you round-trip through the reader.
- **Superscript/subscript** use `^sup^` / `~sub~` (paired, intraword delimiters)
  and `<sup>`/`<sub>`. Because `~` now denotes subscript, a single literal `~`
  forming a pair (e.g. `a~b~c`) becomes subscript; disable with `super_sub: false`.
- **Remote images** (`http(s)://`) are **not** fetched; the alt text is shown as
  a caption instead. Local paths and base64 `data:` URIs are embedded (decoded,
  re-encoded to PNG, and scaled to fit the page). SVG is not supported by the
  image decoder, so it falls back to the caption.
- **Arbitrary HTML** is best-effort: a known subset of inline tags (`<b>`,
  `<i>`, `<u>`, `<s>`, `<code>`, `<mark>`, `<br>`, `<sup>`, `<sub>`, …) maps to
  formatting; other tags are stripped (their text is kept).
- **Footnotes inside footnotes** are not resolved (rendered as `[label]`), and
  links inside a footnote body are flattened to text with the URL appended (a
  real hyperlink there would dangle and trigger a Word repair).
- A footnote referenced multiple times duplicates its body at each reference
  rather than pointing several references at one note.

## Development

```sh
cargo build
cargo test          # 36 integration + 12 unit (math) + 3 doc-tests
cargo run --bin knit-md-docx -- examples/sample.md   # produces examples/sample.docx
```

[`examples/sample.md`](examples/sample.md) exercises every supported feature.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at
your option.
