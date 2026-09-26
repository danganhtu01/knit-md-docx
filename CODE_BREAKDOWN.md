# Code Breakdown — `rust_knit_md_docx` and its `docx-rs` fork

Referenced by: [`MASTER.md`](MASTER.md).

A plain-language tour of every Rust file across both projects, written for a
non-coder. It explains what each file is for and what each function does.

- **`rust_knit_md_docx`** — your crate (the program you run): reads Markdown,
  writes a Word `.docx`.
- **`docx-rs`** (fork: <https://github.com/danganhtu01/knit-md-docx-rs>) — the library
  that physically builds the `.docx`.

---

## Part 1 — The big picture

You have **two projects** that work together:

1. **`rust_knit_md_docx`** (your crate) — the thing you actually run. It reads a
   Markdown file and produces a Word `.docx`. Think of it as a **translator +
   typesetter**: it understands the author's shorthand (Markdown) and lays out a
   properly formatted Word document.
2. **`docx-rs`** (the fork) — a **library** your crate leans on. It knows how to
   physically build a `.docx` file. Your crate decides *what* the document should
   contain; `docx-rs` knows *how* to write it to disk.

One key fact that explains everything: **a `.docx` file is secretly a ZIP folder
full of XML files.** XML is just tagged text, like the HTML behind a web page. So
"writing a Word document" really means "writing a bunch of XML files and zipping
them up." `docx-rs` is the machine that does that; your crate feeds it
instructions.

The flow:

```
Markdown text
   │  (pulldown-cmark library reads it, emits a stream of "events")
   ▼
your engine.rs  ──uses──▶  math.rs (for equations)
   │  (decides: this is a heading, this is a list, this is a table…)
   ▼
docx-rs (the fork)  ──builds──▶  the XML  ──zips──▶  output.docx
```

---

## Part 2 — Your crate, `rust_knit_md_docx` (8 files)

### `lib.rs` — the front desk

This is the **public entrance** to the crate. It doesn't do real work; it offers
convenient ways for other programs to ask for a conversion, then hands the job to
the engine.

- **`to_docx(markdown)`** — "Turn this Markdown text into a Word document object."
  Returns a document you could still tinker with before saving.
- **`to_docx_with(markdown, opts)`** — Same, but with custom settings (page size,
  fonts, etc.).
- **`to_bytes(markdown)`** — "Give me the finished `.docx` as raw bytes" (handy
  for sending over the web without saving to disk). The interesting detail: the
  zip-packer needs to be able to *jump around* while writing, and a plain byte
  list can't do that, so it wraps the bytes in a `Cursor` (a byte list with a
  movable "you are here" marker).
- **`to_bytes_with`** — bytes, with custom settings.
- **`write_file(markdown, output)`** / **`write_file_with`** — "Convert this text
  and save it to this path."
- **`convert_file(input, output)`** — "Read this Markdown *file* and save a
  `.docx`." It also remembers the input file's folder so that images referenced
  relatively (like `![](pic.png)`) can be found.
- **`Converter`** — a reusable object that remembers your settings so you can
  convert many documents without repeating the options each time. Its methods
  (`new`, `with_options`, `options`, `options_mut`, `to_docx`, `to_bytes`,
  `write_file`, `convert_file`) mirror the standalone functions above.
  `convert_file` additionally points the image search folder at the input file's
  directory if you didn't set one.

### `main.rs` — the command-line tool

This turns the crate into a terminal program called `knit-md-docx`.

- **`Cli` (struct)** — the list of options the command accepts (input file,
  `-o output`, `--page a4`, `--smart`, `--no-gfm`, `--body-font`, `--lang de-DE`,
  etc.). The `clap` library reads these from what the user types. The input is
  required unless `--version` (`-V`) is given, which prints the bare version
  number (like `0.2.0`) so an installer can compare it with a release tag.
- **`Page` (enum)** — just the two allowed page sizes: `Letter` or `A4`.
- **`main()`** — the starting point. It reads the command-line arguments, answers
  `--version` itself, otherwise runs the conversion, and prints either "Wrote …" or an error. It returns a
  success/failure code to the operating system.
- **`run(cli)`** — the actual work: read the input (from a file, or from "standard
  input" if you pass `-`), figure out the output filename, assemble the settings
  from the flags, and call `write_file_with`. It returns the output path so `main`
  can report it.

### `error.rs` — how problems are reported

- **`Error` (enum)** — the three kinds of failure: `Io` (couldn't read/write a
  file), `Pack` (the zip step failed), `Input` (the Markdown was unusable).
- **`Display`** — turns an error into a human-readable sentence ("I/O error: …").
- **`source`** / **`From<io::Error>`** — plumbing that lets these errors chain
  nicely and lets file errors auto-convert into our error type.
- **`Result<T>`** — a shorthand meaning "either a successful T, or one of our
  Errors."

### `options.rs` — the settings

- **`PageSetup` (struct)** — page width, height, and margin, measured in **twips**
  (1 twip = 1/1440 of an inch — Word's internal unit). Constants `LETTER` and `A4`
  are pre-filled sizes. `content_width()` / `content_height()` compute the usable
  area (page minus margins) — used later to scale images so they fit.
- **`ConvertOptions` (struct)** — every knob: whether GitHub extensions are on,
  smart punctuation, math, the `native_math` and `super_sub` flags, fonts, body
  size, page setup, and the folder to resolve images against. It also carries
  `lang`, the document language (`None` means: take the front matter's `lang:`,
  else `DEFAULT_LANG`, which is `en-US`). `Default` fills in sensible values (GFM
  on, math on, Calibri body font, Letter page; the command line defaults to A4).
- **`body_half_points()`** — Word measures font size in **half-points**, so this
  doubles your point size (11pt → 22).
- **`cmark_options()`** — translates your settings into the exact switches the
  Markdown parser understands (turn on tables, footnotes, math, superscript,
  etc.). This is the bridge between "your preferences" and "the parser's
  vocabulary."

### `styles.rs` — the look-and-feel

Word documents use named **styles** (like "Heading 1") so the whole document
stays visually consistent. This file defines them once.

- A block of **constants** — the names and colors used throughout: heading style
  IDs, the "Quote" style, the code-block style, the inline-code character style,
  hyperlink style, shading colors (e.g. the light grey behind code), etc.
- **`setup(docx, opts, lang)`** — the only function. It takes a blank document and
  stamps it with: the page size and margins, the default font and size, the
  document language (`w:lang`), no East Asian compatibility flags (why: the
  Language section of [`README.md`](README.md)),
  comfortable line spacing, and the full set of styles (Heading 1–6 with their
  sizes/colors/outline levels, the Quote style, the code style, the caption style,
  inline-code, and hyperlink). After this runs, the document "knows" what a
  heading should look like, so later the engine just says "this paragraph is
  Heading 2" and the appearance is automatic.

### `engine.rs` — the heart of the whole thing

This is where Markdown actually becomes a Word document. The core challenge it
solves: **Markdown is a tree** (a bold word *inside* a list item *inside* a
quote), but **Word is flat** (a document is just a list of paragraphs and tables;
you can't nest things arbitrarily). The engine "flattens the tree" by walking
through the Markdown one piece at a time and keeping a few notebooks (stacks) of
"where am I right now."

The Markdown parser hands the engine a **stream of events**: "start a heading,"
"here's some text," "start bold," "end bold," "start a list," and so on — like
stage directions read aloud in order. The engine reacts to each.

**The data structures (the engine's notebooks):**

- **`Inline`** — one finished small piece of a paragraph: a run of text, a
  hyperlink, or (new) an equation.
- **`BlockOut`** — a finished big piece: a paragraph or a table.
- **`InlineFmt`** — counters for what formatting is currently "switched on": bold,
  italic, strikethrough, underline, highlight, code, superscript, subscript.
  They're *counters* not on/off switches because formatting can nest (bold inside
  bold).
- **`ListCtx` / `ItemCtx`** — track the current list and list item (its number,
  whether it's a checkbox task, etc.).
- **`LinkCtx` / `ImageCtx`** — the link or image currently being built.
- **`TableBuilder`** — accumulates a table cell by cell until it's complete.
- **`Engine`** — holds all of the above plus the growing list of finished blocks,
  the footnote bodies, counters for numbering and bookmarks, and the current quote
  depth.

**The functions, in plain terms:**

- **`build_docx(md, opts)`** — the entry point. It pre-scans footnotes, walks the
  whole event stream, then assembles the final document (styles + numbering
  definitions + all the blocks). Returns the finished document object.
- **`document_lang(md, opts)`** — decides the document language: the option if
  one was given, else a `lang:` line in the front matter (read by
  **`front_matter_lang`**, which only looks inside a leading `---` block that is
  properly closed), else `en-US`. A `de_DE` spelling becomes `de-DE`.
- **`collect_footnotes(md, opts)`** — a *first pass* that renders every footnote's
  body in advance. Word stores a footnote's text at the spot where it's
  referenced, so the engine needs the body ready before it hits the reference.
- **`Engine::new(...)`** — sets up a fresh, empty engine with all notebooks blank.
- **`process_events(iter)`** — the loop: hand each event to `handle`.
- **`finish()`** — at the very end, flush any leftover text into a final paragraph.
- **`handle(ev)`** — the traffic cop. For each event it decides what to do: it
  first swallows footnote-definition events (already handled), captures image
  alt-text if an image is open, and otherwise routes to the right handler (start
  tag, end tag, text, code, math, line break, horizontal rule, etc.).
- **`start_tag(tag)` / `end_tag(tag)`** — react to "something is beginning/ending."
  Starting a heading clears the heading buffer; starting bold bumps the bold
  counter; starting a list registers a new numbering; ending a paragraph flushes
  it into the document; and so on. These two functions are the bulk of the
  structural logic.
- **`on_text(t)`** — plain text arrived. Depending on context it goes into: the
  code buffer, an image's alt-text, the heading title, or — normally — out to the
  text emitter.
- **`emit_text(t)`** — sends text out, with **autolinking done first** (so a bare
  `http://…` URL becomes a clickable link before anything else touches it). *(This
  ordering was a bug fix — see the review notes.)*
- **`emit_scripts(t)`** — scans a chunk of (URL-free) text for `^superscript^` and
  `~subscript~` and emits those pieces with vertical alignment, the rest as normal
  runs. Exists because the Markdown parser only recognizes these at word edges,
  not in the middle of words like `H~2~O`.
- **`emit_script(inner, sup)`** — emit a single super/sub run: flip the counter on,
  make the run, flip it off.
- **`emit_autolinked(t)`** — find bare URLs in text and turn them into hyperlinks;
  hands the non-URL gaps to `emit_scripts`. Inside an existing link it skips
  URL-hunting.
- **`on_soft_break()`** — a single newline in the source becomes either a space or
  a real line break, per your settings.
- **`styled_run(text, is_code)`** — the workhorse that builds **one run of text**
  stamped with whatever formatting is currently switched on (bold, italic,
  superscript, code shading, …). A "run" is Word's term for a stretch of text that
  all looks the same.
- **`emit_run(r)`** — files a finished run into the right place: the open link, the
  open table cell, or the current paragraph.
- **`target_buf()`** — answers "where should the next piece go?" (a table cell if
  one is open, otherwise the paragraph being built).
- **`flush_text_paragraph()`** — take all the accumulated runs and turn them into a
  finished paragraph, applying list/quote decoration. ("Flush" = "finalize and
  file away.")
- **`quote_indent()` / `list_indent()`** — compute how far to indent, based on how
  deeply nested in quotes/lists you are.
- **`decorate(p)`** — apply list numbering and quote styling/indent to a paragraph.
  It carefully combines list + quote indent into one setting so they don't
  overwrite each other.
- **`flush_heading()`** — finalize a heading: apply the Heading-N style, and (if
  enabled) drop a **bookmark** so links like `[jump](#my-section)` can target it.
- **`flush_link()`** — finalize a hyperlink. Inside footnotes it "flattens" the
  link to plain text with the URL appended (a real link there would corrupt the
  file).
- **`flush_image()`** — finalize an image: try to load and embed it; if that fails,
  fall back to a caption like `[image: alt text]` instead of crashing.
- **`flush_code_block()`** — turn a fenced code block into a shaded, full-width box
  (implemented as a borderless one-cell table) with indentation preserved.
- **`flush_table_cell()` / `flush_table()`** — finalize a table cell (with
  alignment and header shading) and then the whole table (sizing columns to fit
  the page).
- **`flush_def()`** — finalize a definition-list entry (bold term, indented
  definition).
- **`push_alert_label(kind)`** — emit the colored "Note / Warning / …" label for
  GitHub-style alert quotes.
- **`push_hr()`** — a thematic break (`---`): an empty paragraph with a **bottom
  border**, which Word draws as a horizontal line. *(New — used to be a row of em
  dashes.)*
- **`math_run(src)`** — the *fallback* for math: render the LaTeX as italic
  "Cambria Math" text (used when native equations are off, or inside a link).
- **`on_inline_math(src)` / `on_display_math(src)`** — handle `$…$` and `$$…$$`.
  Normally they call the math translator to produce a **native Word equation**;
  inside a link or with native math disabled, they fall back to text. Display math
  becomes its own centered paragraph.
- **`push_footnote_ref(label)`** — at a `[^1]` reference, attach the pre-rendered
  footnote body as a real Word footnote.
- **`on_html_block(s)` / `on_inline_html(s)`** — best-effort handling of raw HTML:
  block HTML is stripped to its text; inline tags like `<b>`, `<mark>`, `<sup>`
  flip the matching formatting counters.
- **`alloc_numbering(ordered, level, start)`** — register a fresh numbering scheme
  for each list, so ordered lists restart correctly and nested lists indent
  independently. (It deliberately starts IDs at 2 because Word reserves ID 1.)
- **`next_bookmark()` / `unique_anchor(base)`** — hand out unique bookmark IDs and
  unique anchor names (so two "Introduction" headings don't collide).
- **`load_image(img)`** — load an image from a local file or an embedded `data:`
  URI, decode it, re-encode to PNG, reject absurd sizes, and scale it to fit the
  page. Returns nothing on failure (caller shows a caption). Remote `http://`
  images are *not* downloaded.
- **`resolve_path(url)`** — turn a relative image path into a full path using the
  base folder.

**The free helpers at the bottom:**

- **`checkbox_run(checked)`** — produces the ☒ or ☐ glyph for task lists.
- **`script_span(t, open, delim)`** — given a `^` or `~`, find its matching closing
  delimiter (no whitespace between, not a doubled `~~`). Returns where the closing
  mark is, or nothing.
- **`bullet_glyph(level)`** — picks the bullet character (•, ◦, ▪) by nesting
  depth.
- **`slugify(s)`** — turns a heading title into a URL-style slug ("My Section" →
  "my-section") so anchor links match.
- **`bookmark_name(slug)`** — sanitizes a slug into a name Word accepts as a
  bookmark.
- **`normalize_link(url)`** — adds `mailto:` to bare email addresses.
- **`strip_html_tags(s)`** — crudely removes `<tags>`, keeping the text between
  them.

### `math.rs` — the LaTeX-to-equation translator (new)

This file reads LaTeX math like `\frac{-b \pm \sqrt{b^2-4ac}}{2a}` and builds a
**structured equation** Word can display and edit. It's a small **recursive-descent
parser** — meaning it reads left to right, and when it hits something with inner
parts (like a fraction's top and bottom), it calls *itself* to handle those parts.

- **`latex_to_omath(src, display)`** — the entry point: parse the LaTeX into
  equation pieces, wrap them as an equation (block or inline). If parsing yields
  nothing, it keeps the raw text so nothing is lost.
- **`Parser` (struct) + `new`** — holds the characters and a "you are here"
  position.
- **`peek()` / `bump()` / `eat(ch)`** — look at the current character / consume it
  and advance / consume it *only if* it matches (a safety helper added during the
  review).
- **`parse_sequence(stop, in_delim)`** — reads a run of pieces until it hits a
  stopping character (like `}`). This is also where the **depth guard** lives: if
  nesting gets pathologically deep (an attack), it stops descending and dumps the
  rest as plain text instead of crashing.
- **`parse_scripted_atom()`** — read one piece *and* any `^`/`_` attached to it.
- **`attach_scripts(base)`** — wrap a piece in superscript/subscript/both if
  `^`/`_` follow it (`x^2`, `x_i`, `x_i^2`).
- **`parse_script_arg()` / `parse_group_arg()`** — read the argument after a `^`/`_`
  or a command — either a single token or a whole `{…}` group.
- **`parse_atom()`** — read one atom: a `{group}`, a `\command`, or a single
  character.
- **`parse_command()`** — handle a `\command`: fractions, square roots,
  `\left(…\right)`, sums/integrals, or a symbol/Greek letter/function name. Unknown
  commands are kept as literal text so nothing disappears.
- **`parse_delimited()` / `read_delimiter()`** — handle `\left( … \right)` bracket
  groups.
- **`parse_nary(op)`** — handle big operators like `\sum`/`\int`, attaching their
  lower/upper limits and the thing they apply to.
- **`read_command_name()` / `skip_spaces()` / `looks_at_command(word)`** — small
  readers: grab a command's letters / skip spaces / peek whether `\word` is next.
- **`merge_runs(...)` / `recurse_merge(...)`** — tidy the result by gluing adjacent
  plain-text pieces into one (so `x+y` is one run, not three), recursively through
  all the nested parts.
- **`nary_operator(cmd)`** — maps `\sum`→∑, `\int`→∫, etc.
- **`control_symbol(c)`** — maps backslash-punctuation like `\{`, `\%`, `\,` (a thin
  space).
- **`command_text(name)`** — the big lookup table: every Greek letter, operator,
  relation, arrow, and function name → its Unicode character (`\alpha`→α, `\leq`→≤,
  `\rightarrow`→→, `\sin`→"sin").

### `tests/conversion.rs` — the proof it works

More than 40 automated checks. Each one converts a snippet of Markdown, unzips the
resulting `.docx`, and verifies the XML contains what it should: headings get
heading styles, lists produce numbering, tables have aligned columns, math
produces equation XML, `^x^` produces superscript, metacharacters are escaped,
deeply-nested math doesn't crash, a URL with a caret isn't broken, the
document language comes from the option or the front matter, the East Asian
flags are gone, and so on.
These run automatically and fail loudly if a future change breaks something.

---

## Part 3 — The fork, `docx-rs` (338 files)

I won't list 338 files individually — it would be pages of noise, because the fork
is a library built almost entirely from **one repeating template**. Here's how to
understand the *whole thing* without that.

### How the fork is organized (the five neighborhoods)

| Folder | Role | Plain-language analogy |
|---|---|---|
| **`documents/elements/`** (145 files) | One file per Word building block (paragraph, run, table, image, bold, color…) | The **parts catalog** — every kind of Lego brick |
| **`xml_builder/`** | Low-level helpers that write actual XML tags | The **printer** that puts ink on the page |
| **`reader/`** (≈120 files) | Reads an existing `.docx` back into objects | The **scanner** (the reverse direction) |
| **`types/`** | Simple option lists (alignment, border styles, units) | The **dropdown menus** of allowed values |
| **`xml/`, `zipper/`, `escape/`** | The plumbing: raw XML writing, zipping, escaping special characters | The **factory floor** |

**The repeating template** that ~145 of those files follow: each "element" file
(say, `bold.rs`) defines (1) a little data structure, and (2) a `build_to`
function that writes that element's XML tag. Once you understand `bold.rs`, you
understand the pattern behind 145 of them — they only differ in which tag they
write. That's why I'm not enumerating each.

The two ends of the library:

- **Writer** (your crate uses this): you assemble elements, call `.build()`, and it
  walks the tree calling each element's `build_to` to produce XML, then zips it.
  The interesting helper is **`escape()`** in `escape/mod.rs` — it converts
  dangerous characters (`<`, `>`, `&`) into safe codes (`&lt;` …) so they don't
  corrupt the XML. (This is exactly the function our critical bug fix reached for.)
- **Reader** (your crate doesn't use this): the reverse, for loading existing
  documents. *This is why equations are "write-only"* — the reader has no code to
  understand `m:oMath`, so a load-then-save round-trip through the reader would drop
  them.

### The files we actually added or changed in the fork

These five are the only ones that matter for your three features:

- **`documents/elements/omath.rs`** *(new — the big one)*. Defines a Word
  **equation**.
  - **`OMathElement` (enum)** — every kind of math piece: a plain run,
    super/subscript, fraction, radical (root), n-ary operator (∑/∫), bracket group,
    and function. This is the in-memory shape of an equation.
  - **`OMath` (struct)** with **`new`** (a one-text equation), **`from_elements`**
    (a structured one), and **`display`** (mark it as a centered block).
  - **`build_to`** functions — walk the equation tree and write the corresponding
    `m:…` XML tags (`m:sSup` for superscript, `m:f` for fraction, etc.). The
    `build_elements` / `build_arg` helpers handle the repetitive "write a slot of
    child pieces" work. **Our escaping fix lives here:** the text and the
    bracket/operator characters now pass through `escape()` so `<`, `>`, `&` in
    math can't break the file.
- **`documents/elements/run.rs`** — we added **`Run::superscript()`** and
  **`Run::subscript()`**: two one-line methods that tag a run as raised/lowered
  text. (The underlying capability already existed; it just wasn't reachable.)
- **`documents/elements/paragraph.rs`** — we added: the **`OMath` variant** to the
  list of things a paragraph can contain, **`add_omath()`** (put an equation in a
  paragraph), and **`set_borders()` / `set_border()` / `clear_border()`** (which our
  horizontal-rule feature uses). We also extended the two internal functions that
  walk a paragraph's children (`build_to` for XML output and the serializer) to
  know about equations.
- **`documents/elements/paragraph_borders.rs`** — *not changed*, but this is the
  existing machinery our `set_borders` exposes: it defines a paragraph's four
  borders and writes them as the `w:pBdr` XML. We just opened a door to it.
- **`xml_builder/document.rs`** — a one-line-ish change: we added the **`xmlns:m`**
  declaration to the document's root tag. That's the namespace announcement that
  tells Word "this file may contain math (`m:`) elements." Without it, Word rejects
  every equation.

The document-language work (2026-09-24) added or changed these:

- **`documents/elements/lang.rs`** *(new)* — **`Lang`**, the `w:lang` tag that
  says which language a run's text is in (`val` for Latin text, plus optional
  `east_asia` and `bidi`). Word and LibreOffice pick spelling, hyphenation and
  line breaking by it.
- **`run_property.rs`**, **`run.rs`**, **`run_property_default.rs`**,
  **`doc_defaults.rs`**, **`styles.rs`** and **`documents/mod.rs`** — a `lang(...)`
  method at each level, up to **`Docx::default_lang()`**, the one this crate uses.
- **`documents/settings.rs`** — **`east_asian_compat(bool)`**, a switch for five
  compatibility flags a Japanese Word template writes and upstream always emitted
  (`balanceSingleByteDoubleByteWidth`, `useFELayout` and three more). On by
  default as upstream; this crate turns it off, through
  **`Docx::east_asian_compat(false)`**.
- **`reader/run_property.rs`**, **`reader/xml_element.rs`** — reading `w:lang`
  back from an existing `.docx`.

### Why this split (your crate vs. the fork) matters

Your crate is the **decision-maker** — it reads Markdown and decides "this is a
fraction, this heading needs a bookmark, this image scales to here." The fork is
the **builder** — it knows nothing about Markdown; it just faithfully writes
whatever Word XML it's told to. The three features needed the *builder* to learn
three new tricks (equations, raised/lowered text, paragraph borders), which is
exactly why we forked it. Everything else — interpreting your Markdown — lives in
your crate's `engine.rs` and `math.rs`.
