//! The conversion engine: a fold over `pulldown-cmark`'s balanced event stream
//! that flattens the Markdown tree into a flat sequence of Word paragraphs and
//! tables.
//!
//! Word has no nested-content model — runs cannot nest and the body is a flat
//! list of blocks — so the engine keeps explicit stacks (inline formatting,
//! lists, block quotes, the open link/image, a table builder) and *materialises
//! a fully-resolved run at every leaf inline event*, stamping it with the
//! cumulative formatting that is live at that moment.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use docx_rs::*;
use image::GenericImageView;
use pulldown_cmark::{
    Alignment, BlockQuoteKind, CowStr, Event, Parser, Tag, TagEnd, TextMergeStream,
};

use crate::options::ConvertOptions;
use crate::styles;

/// A finished, fully-resolved inline element belonging to the paragraph being
/// built. A hyperlink is a paragraph child in OOXML (not a run), so it cannot
/// live inside the run list — hence the enum. `Run` is the hot, common variant
/// (left unboxed); the larger, rarer `Hyperlink` is boxed.
#[allow(clippy::large_enum_variant)]
enum Inline {
    Run(Run),
    Link(Box<Hyperlink>),
    /// A native Word equation (`m:oMath`). Like a hyperlink it is a paragraph
    /// child rather than a run, so it cannot live in the run list.
    Math(Box<OMath>),
}

/// A finished block destined for the document body.
#[allow(clippy::large_enum_variant)]
enum BlockOut {
    Para(Paragraph),
    Table(Box<Table>),
}

/// Cumulative inline formatting. Counters (not booleans) because the same flag
/// nests, e.g. `**a *b* c**` or `***x***`.
#[derive(Clone, Copy, Default)]
struct InlineFmt {
    bold: u32,
    italic: u32,
    strike: u32,
    underline: u32,
    highlight: u32,
    code: u32,
    superscript: u32,
    subscript: u32,
}

/// One frame per open Markdown list.
#[derive(Clone, Copy)]
struct ListCtx {
    num_id: usize,
    level: usize,
}

/// One frame per open list item.
struct ItemCtx {
    task: Option<bool>,
    numbered: bool,
    task_emitted: bool,
}

/// The currently-open link (between `Start(Link)` and `End(Link)`).
struct LinkCtx {
    anchor: bool,
    target: String,
    runs: Vec<Run>,
}

/// The currently-open image (between `Start(Image)` and `End(Image)`); inner
/// text events accumulate as alt text.
struct ImageCtx {
    url: String,
    alt: String,
}

/// Accumulates a GFM table until `End(Table)`.
struct TableBuilder {
    aligns: Vec<Alignment>,
    rows: Vec<Vec<TableCell>>,
    cur_row: Vec<TableCell>,
    cur_cell: Option<Vec<Inline>>,
    col: usize,
    in_head: bool,
}

/// Which kind of definition-list item is currently open.
#[derive(Clone, Copy)]
enum DefKind {
    Title,
    Definition,
}

/// The walk state.
struct Engine<'a> {
    opts: &'a ConvertOptions,
    footnotes: HashMap<String, Vec<Paragraph>>,

    blocks: Vec<BlockOut>,
    abstracts: Vec<AbstractNumbering>,
    numberings: Vec<Numbering>,
    num_counter: usize,
    bookmark_counter: usize,
    used_anchors: HashSet<String>,

    fmt: InlineFmt,
    pending: Vec<Inline>,
    link: Option<LinkCtx>,
    image: Option<ImageCtx>,

    list_stack: Vec<ListCtx>,
    item_stack: Vec<ItemCtx>,
    quote_depth: usize,

    heading: Option<usize>,
    heading_id: Option<String>,
    heading_text: String,

    in_code: bool,
    code_buf: String,

    table: Option<TableBuilder>,
    in_metadata: bool,
    fn_skip: usize,

    /// When true (footnote sub-render), links are flattened to plain runs so
    /// they never allocate an `r:id` that footnotes.xml.rels would not register.
    flatten_links: bool,
}

/// Public entry point used by `lib.rs`: build a `Docx` from Markdown.
pub(crate) fn build_docx(md: &str, opts: &ConvertOptions) -> Docx {
    let footnotes = collect_footnotes(md, opts);

    let mut eng = Engine::new(opts, footnotes);
    let parser = TextMergeStream::new(Parser::new_ext(md, opts.cmark_options()));
    eng.process_events(parser);
    eng.finish();

    let mut docx = styles::setup(Docx::new(), opts);
    for a in std::mem::take(&mut eng.abstracts) {
        docx = docx.add_abstract_numbering(a);
    }
    for n in std::mem::take(&mut eng.numberings) {
        docx = docx.add_numbering(n);
    }
    for b in std::mem::take(&mut eng.blocks) {
        docx = match b {
            BlockOut::Para(p) => docx.add_paragraph(p),
            BlockOut::Table(t) => docx.add_table(*t),
        };
    }
    docx
}

/// Pre-pass: render every footnote *definition* body to paragraphs, keyed by
/// label, so the main pass can attach them at each reference site (Word
/// footnotes carry their content at the reference, not in a trailing section).
fn collect_footnotes(md: &str, opts: &ConvertOptions) -> HashMap<String, Vec<Paragraph>> {
    let mut map = HashMap::new();
    let parser = TextMergeStream::new(Parser::new_ext(md, opts.cmark_options()));

    let mut label: Option<String> = None;
    let mut buf: Vec<Event> = Vec::new();
    let mut depth = 0usize;

    for ev in parser {
        if label.is_some() {
            match &ev {
                Event::Start(Tag::FootnoteDefinition(_)) => {
                    depth += 1;
                    buf.push(ev);
                }
                Event::End(TagEnd::FootnoteDefinition) => {
                    depth -= 1;
                    if depth == 0 {
                        let lbl = label.take().unwrap();
                        let events = std::mem::take(&mut buf);
                        let mut sub = Engine::new(opts, HashMap::new());
                        // Links inside footnote bodies must not become real
                        // hyperlinks (their r:id would dangle in footnotes.xml).
                        sub.flatten_links = true;
                        sub.process_events(events.into_iter());
                        sub.finish();
                        let paras: Vec<Paragraph> = sub
                            .blocks
                            .into_iter()
                            .filter_map(|b| match b {
                                BlockOut::Para(p) => Some(p),
                                BlockOut::Table(_) => None,
                            })
                            .collect();
                        map.insert(lbl, paras);
                    } else {
                        buf.push(ev);
                    }
                }
                _ => buf.push(ev),
            }
        } else if let Event::Start(Tag::FootnoteDefinition(name)) = &ev {
            label = Some(name.to_string());
            depth = 1;
            buf.clear();
        }
    }
    map
}

impl<'a> Engine<'a> {
    fn new(opts: &'a ConvertOptions, footnotes: HashMap<String, Vec<Paragraph>>) -> Self {
        Engine {
            opts,
            footnotes,
            blocks: Vec::new(),
            abstracts: Vec::new(),
            numberings: Vec::new(),
            num_counter: 0,
            bookmark_counter: 0,
            used_anchors: HashSet::new(),
            fmt: InlineFmt::default(),
            pending: Vec::new(),
            link: None,
            image: None,
            list_stack: Vec::new(),
            item_stack: Vec::new(),
            quote_depth: 0,
            heading: None,
            heading_id: None,
            heading_text: String::new(),
            in_code: false,
            code_buf: String::new(),
            table: None,
            in_metadata: false,
            fn_skip: 0,
            flatten_links: false,
        }
    }

    fn process_events<'e, I: Iterator<Item = Event<'e>>>(&mut self, iter: I) {
        for ev in iter {
            self.handle(ev);
        }
    }

    /// Flush anything still buffered at end of input.
    fn finish(&mut self) {
        if !self.pending.is_empty() {
            self.flush_text_paragraph();
        }
    }

    fn handle(&mut self, ev: Event<'_>) {
        // Footnote definition bodies are pre-rendered (see `collect_footnotes`);
        // swallow them (and everything nested inside) in the main pass.
        match &ev {
            Event::Start(Tag::FootnoteDefinition(_)) => {
                self.fn_skip += 1;
                return;
            }
            Event::End(TagEnd::FootnoteDefinition) => {
                self.fn_skip = self.fn_skip.saturating_sub(1);
                return;
            }
            _ => {}
        }
        if self.fn_skip > 0 {
            return;
        }

        // While an image is open, every inline leaf is part of its alt text, not
        // body content. Capture text-bearing events and swallow the rest so they
        // cannot escape the image and corrupt the surrounding paragraph.
        if let Some(image) = self.image.as_mut() {
            match &ev {
                Event::Text(t) | Event::Code(t) | Event::InlineMath(t) => {
                    image.alt.push_str(t);
                    return;
                }
                Event::SoftBreak | Event::HardBreak => {
                    image.alt.push(' ');
                    return;
                }
                // The matching End(Image) must still be processed below.
                Event::End(TagEnd::Image) => {}
                _ => return,
            }
        }

        match ev {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(t) => self.on_text(&t),
            Event::Code(t) => {
                let r = self.styled_run(&t, true);
                self.emit_run(r);
            }
            Event::InlineMath(t) => self.on_inline_math(&t),
            Event::DisplayMath(t) => self.on_display_math(&t),
            Event::Html(s) => self.on_html_block(&s),
            Event::InlineHtml(s) => self.on_inline_html(&s),
            Event::FootnoteReference(label) => self.push_footnote_ref(&label),
            Event::SoftBreak => self.on_soft_break(),
            Event::HardBreak => {
                if self.image.is_none() {
                    let r = Run::new().add_break(BreakType::TextWrapping);
                    self.emit_run(r);
                }
            }
            Event::Rule => {
                self.flush_text_paragraph();
                self.push_hr();
            }
            Event::TaskListMarker(checked) => {
                if let Some(it) = self.item_stack.last_mut() {
                    it.task = Some(checked);
                }
            }
        }
    }

    // ---- block starts ------------------------------------------------------

    fn start_tag(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {}
            Tag::Heading { level, id, .. } => {
                self.flush_text_paragraph();
                self.heading = Some(level as usize);
                self.heading_id = id.map(|c| c.to_string());
                self.heading_text.clear();
            }
            Tag::BlockQuote(kind) => {
                self.flush_text_paragraph();
                self.quote_depth += 1;
                if let Some(k) = kind {
                    self.push_alert_label(k);
                }
            }
            Tag::CodeBlock(_) => {
                self.flush_text_paragraph();
                self.in_code = true;
                self.code_buf.clear();
            }
            Tag::List(start) => {
                self.flush_text_paragraph();
                let level = self.list_stack.len();
                let num_id = self.alloc_numbering(start.is_some(), level, start.unwrap_or(1));
                self.list_stack.push(ListCtx { num_id, level });
            }
            Tag::Item => {
                self.item_stack.push(ItemCtx {
                    task: None,
                    numbered: false,
                    task_emitted: false,
                });
            }
            Tag::Table(aligns) => {
                self.flush_text_paragraph();
                self.table = Some(TableBuilder {
                    aligns,
                    rows: Vec::new(),
                    cur_row: Vec::new(),
                    cur_cell: None,
                    col: 0,
                    in_head: false,
                });
            }
            Tag::TableHead => {
                if let Some(t) = self.table.as_mut() {
                    t.in_head = true;
                    t.cur_row.clear();
                    t.col = 0;
                }
            }
            Tag::TableRow => {
                if let Some(t) = self.table.as_mut() {
                    t.cur_row.clear();
                    t.col = 0;
                }
            }
            Tag::TableCell => {
                if let Some(t) = self.table.as_mut() {
                    t.cur_cell = Some(Vec::new());
                }
            }
            Tag::Emphasis => self.fmt.italic += 1,
            Tag::Strong => self.fmt.bold += 1,
            Tag::Strikethrough => self.fmt.strike += 1,
            Tag::Superscript => self.fmt.superscript += 1,
            Tag::Subscript => self.fmt.subscript += 1,
            Tag::Link { dest_url, .. } => {
                let s = dest_url.to_string();
                let (anchor, target) = if let Some(rest) = s.strip_prefix('#') {
                    (true, bookmark_name(&slugify(rest)))
                } else {
                    (false, normalize_link(&s))
                };
                self.link = Some(LinkCtx {
                    anchor,
                    target,
                    runs: Vec::new(),
                });
            }
            Tag::Image { dest_url, .. } => {
                self.image = Some(ImageCtx {
                    url: dest_url.to_string(),
                    alt: String::new(),
                });
            }
            Tag::HtmlBlock => {}
            Tag::MetadataBlock(_) => self.in_metadata = true,
            Tag::DefinitionList => self.flush_text_paragraph(),
            Tag::DefinitionListTitle => self.flush_text_paragraph(),
            Tag::DefinitionListDefinition => self.flush_text_paragraph(),
            Tag::FootnoteDefinition(_) => {}
        }
    }

    // ---- block ends --------------------------------------------------------

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.flush_text_paragraph(),
            TagEnd::Heading(_) => self.flush_heading(),
            TagEnd::BlockQuote(_) => {
                self.flush_text_paragraph();
                self.quote_depth = self.quote_depth.saturating_sub(1);
            }
            TagEnd::CodeBlock => self.flush_code_block(),
            TagEnd::List(_) => {
                self.list_stack.pop();
            }
            TagEnd::Item => {
                self.flush_text_paragraph();
                self.item_stack.pop();
            }
            TagEnd::Table => self.flush_table(),
            TagEnd::TableHead => {
                if let Some(t) = self.table.as_mut() {
                    let row = std::mem::take(&mut t.cur_row);
                    t.rows.push(row);
                    t.in_head = false;
                }
            }
            TagEnd::TableRow => {
                if let Some(t) = self.table.as_mut() {
                    let row = std::mem::take(&mut t.cur_row);
                    t.rows.push(row);
                }
            }
            TagEnd::TableCell => self.flush_table_cell(),
            TagEnd::Emphasis => self.fmt.italic = self.fmt.italic.saturating_sub(1),
            TagEnd::Strong => self.fmt.bold = self.fmt.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.fmt.strike = self.fmt.strike.saturating_sub(1),
            TagEnd::Superscript => self.fmt.superscript = self.fmt.superscript.saturating_sub(1),
            TagEnd::Subscript => self.fmt.subscript = self.fmt.subscript.saturating_sub(1),
            TagEnd::Link => self.flush_link(),
            TagEnd::Image => self.flush_image(),
            TagEnd::HtmlBlock => {}
            TagEnd::MetadataBlock(_) => self.in_metadata = false,
            TagEnd::DefinitionList => {}
            TagEnd::DefinitionListTitle => self.flush_def(DefKind::Title),
            TagEnd::DefinitionListDefinition => self.flush_def(DefKind::Definition),
            TagEnd::FootnoteDefinition => {}
        }
    }

    // ---- inline leaves -----------------------------------------------------

    fn on_text(&mut self, t: &str) {
        if self.in_metadata {
            return;
        }
        if self.in_code {
            self.code_buf.push_str(t);
            return;
        }
        if let Some(img) = self.image.as_mut() {
            img.alt.push_str(t);
            return;
        }
        if self.heading.is_some() {
            self.heading_text.push_str(t);
        }
        self.emit_text(t);
    }

    /// Emit body text. Autolinking is the *outer* pass: bare URLs are located
    /// over the whole string and emitted whole, and only the URL-free gaps are
    /// handed to the intraword `^superscript^` / `~subscript~` scanner. (Doing it
    /// the other way round let a `^x^` / `~x~` inside a URL split the link.)
    fn emit_text(&mut self, t: &str) {
        self.emit_autolinked(t);
    }

    /// Peel off intraword `^superscript^` and `~subscript~` spans (which
    /// pulldown-cmark only recognises at word boundaries) from a slice that is
    /// already known to be URL-free, emitting the remainder as plain runs.
    fn emit_scripts(&mut self, t: &str) {
        if !self.opts.super_sub {
            let r = self.styled_run(t, false);
            self.emit_run(r);
            return;
        }
        let bytes = t.as_bytes();
        let mut i = 0;
        let mut seg_start = 0;
        while i < t.len() {
            let b = bytes[i];
            if (b == b'^' || b == b'~') && i + 1 < t.len() {
                if let Some(end) = script_span(t, i, b) {
                    if seg_start < i {
                        let r = self.styled_run(&t[seg_start..i], false);
                        self.emit_run(r);
                    }
                    let inner = &t[i + 1..end];
                    self.emit_script(inner, b == b'^');
                    i = end + 1;
                    seg_start = i;
                    continue;
                }
            }
            i += 1;
        }
        if seg_start < t.len() {
            let r = self.styled_run(&t[seg_start..], false);
            self.emit_run(r);
        }
    }

    /// Emit one run of script (super/sub) text with the matching vertical
    /// alignment applied for the duration of the run.
    fn emit_script(&mut self, inner: &str, superscript: bool) {
        if superscript {
            self.fmt.superscript += 1;
        } else {
            self.fmt.subscript += 1;
        }
        let r = self.styled_run(inner, false);
        self.emit_run(r);
        if superscript {
            self.fmt.superscript -= 1;
        } else {
            self.fmt.subscript -= 1;
        }
    }

    /// Emit text, turning bare `http(s)://` URLs into hyperlinks (GFM extended
    /// autolinking, which pulldown-cmark does not perform). Suppressed inside an
    /// explicit link and in footnote bodies (where links are flattened).
    fn emit_autolinked(&mut self, t: &str) {
        if self.link.is_some() || self.flatten_links {
            // Inside an explicit link / footnote body: no autolinking, but still
            // honour intraword scripts.
            self.emit_scripts(t);
            return;
        }
        let mut last = 0;
        let mut search = 0;
        while let Some(rel) = t[search..].find("http") {
            let s = search + rel;
            let rest = &t[s..];
            let is_scheme = rest.starts_with("http://") || rest.starts_with("https://");
            let boundary_ok = s == 0
                || !t[..s]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric());
            if is_scheme && boundary_ok {
                let mut e = s;
                for (off, ch) in rest.char_indices() {
                    if ch.is_whitespace() || matches!(ch, '<' | '>' | '"' | '`' | '|') {
                        break;
                    }
                    e = s + off + ch.len_utf8();
                }
                // Strip trailing punctuation that is unlikely to be part of the URL.
                while e > s {
                    let c = t[..e].chars().next_back().unwrap();
                    if matches!(
                        c,
                        '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '\'' | '"'
                    ) {
                        e -= c.len_utf8();
                    } else {
                        break;
                    }
                }
                let scheme_len = if rest.starts_with("https://") { 8 } else { 7 };
                if e > s + scheme_len {
                    if last < s {
                        self.emit_scripts(&t[last..s]);
                    }
                    let url = t[s..e].to_string();
                    let run = self.styled_run(&url, false).style(styles::HYPERLINK);
                    let h = Hyperlink::new(url, HyperlinkType::External).add_run(run);
                    self.target_buf().push(Inline::Link(Box::new(h)));
                    last = e;
                    search = e;
                    continue;
                }
            }
            search = s + 4;
        }
        if last < t.len() {
            self.emit_scripts(&t[last..]);
        }
    }

    fn on_soft_break(&mut self) {
        if self.in_metadata || self.image.is_some() {
            return;
        }
        if self.heading.is_some() {
            self.heading_text.push(' ');
        }
        let r = if self.opts.soft_breaks_as_newlines {
            Run::new().add_break(BreakType::TextWrapping)
        } else {
            self.styled_run(" ", false)
        };
        self.emit_run(r);
    }

    /// Build a single run carrying the current cumulative formatting.
    fn styled_run(&self, text: &str, is_code: bool) -> Run {
        let mut r = Run::new().add_text(text.to_string());
        if self.fmt.bold > 0 {
            r = r.bold();
        }
        if self.fmt.italic > 0 {
            r = r.italic();
        }
        if self.fmt.strike > 0 {
            r = r.strike();
        }
        if self.fmt.underline > 0 {
            r = r.underline("single");
        }
        if self.fmt.highlight > 0 {
            r = r.highlight("yellow");
        }
        // Vertical alignment is mutually exclusive; an outer superscript wins.
        if self.fmt.superscript > 0 {
            r = r.superscript();
        } else if self.fmt.subscript > 0 {
            r = r.subscript();
        }
        if self.table.as_ref().is_some_and(|t| t.in_head) {
            r = r.bold();
        }
        if is_code || self.fmt.code > 0 {
            r = r
                .fonts(
                    RunFonts::new()
                        .ascii(self.opts.code_font.clone())
                        .hi_ansi(self.opts.code_font.clone()),
                )
                .style(styles::VERBATIM_CHAR)
                .shading(
                    Shading::new()
                        .shd_type(ShdType::Clear)
                        .color("auto")
                        .fill(self.opts.inline_code_fill.clone()),
                );
        }
        r
    }

    /// Route a run to the active inline sink: the open link, else the open
    /// table cell, else the current paragraph buffer.
    fn emit_run(&mut self, r: Run) {
        if let Some(link) = self.link.as_mut() {
            link.runs.push(r);
        } else {
            self.target_buf().push(Inline::Run(r));
        }
    }

    fn target_buf(&mut self) -> &mut Vec<Inline> {
        let in_cell = self.table.as_ref().is_some_and(|t| t.cur_cell.is_some());
        if in_cell {
            self.table.as_mut().unwrap().cur_cell.as_mut().unwrap()
        } else {
            &mut self.pending
        }
    }

    // ---- flushers ----------------------------------------------------------

    fn flush_text_paragraph(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let mut prefix = None;
        if let Some(item) = self.item_stack.last_mut() {
            if let Some(checked) = item.task {
                if !item.task_emitted {
                    prefix = Some(checkbox_run(checked));
                    item.task_emitted = true;
                }
            }
        }
        let mut p = Paragraph::new();
        if let Some(r) = prefix {
            p = p.add_run(r);
        }
        for inl in std::mem::take(&mut self.pending) {
            p = match inl {
                Inline::Run(r) => p.add_run(r),
                Inline::Link(h) => p.add_hyperlink(*h),
                Inline::Math(m) => p.add_omath(*m),
            };
        }
        p = self.decorate(p);
        self.blocks.push(BlockOut::Para(p));
    }

    /// Left indent contributed by the open block-quote nesting, in twips.
    fn quote_indent(&self) -> i32 {
        if self.quote_depth > 0 {
            (self.quote_depth as i32)
                .saturating_mul(360)
                .saturating_add(360)
        } else {
            0
        }
    }

    /// Left indent contributed by the open list nesting, in twips.
    fn list_indent(&self) -> i32 {
        self.list_stack.last().map_or(0, |l| {
            (l.level as i32).saturating_add(1).saturating_mul(720)
        })
    }

    /// Apply list numbering and block-quote styling to a body paragraph. The
    /// list and quote indents are combined into a single `.indent` call so the
    /// two do not clobber each other (docx-rs keeps only the last `indPr`).
    fn decorate(&mut self, mut p: Paragraph) -> Paragraph {
        let mut numbered = false;
        if let Some(list) = self.list_stack.last().copied() {
            if let Some(item) = self.item_stack.last_mut() {
                if item.task.is_none() && !item.numbered {
                    p = p.numbering(NumberingId::new(list.num_id), IndentLevel::new(0));
                    item.numbered = true;
                    numbered = true;
                }
            }
        }
        if self.quote_depth > 0 {
            p = p.style(styles::QUOTE);
        }
        // A numbered paragraph already carries indent from its numbering level;
        // adding more would fight it. Everything else gets the combined indent.
        if !numbered {
            let indent = self.list_indent().saturating_add(self.quote_indent());
            if indent > 0 {
                p = p.indent(Some(indent), None, None, None);
            }
        }
        p
    }

    fn flush_heading(&mut self) {
        let lvl = self.heading.take().unwrap_or(1).clamp(1, 6);
        let style = styles::HEADING[lvl - 1];

        let mut p = Paragraph::new();
        let mut bid = None;
        if self.opts.heading_anchors {
            // Slugify the explicit `{#id}` too, so it matches the link side
            // (which always slugifies `#target`).
            let raw = self
                .heading_id
                .take()
                .filter(|s| !s.is_empty())
                .map(|id| slugify(&id))
                .unwrap_or_else(|| slugify(&self.heading_text));
            let name = self.unique_anchor(bookmark_name(&raw));
            let id = self.next_bookmark();
            p = p.add_bookmark_start(id, name);
            bid = Some(id);
        }
        for inl in std::mem::take(&mut self.pending) {
            p = match inl {
                Inline::Run(r) => p.add_run(r),
                Inline::Link(h) => p.add_hyperlink(*h),
                Inline::Math(m) => p.add_omath(*m),
            };
        }
        if let Some(id) = bid {
            p = p.add_bookmark_end(id);
        }
        p = p.style(style);
        // Honour list/quote nesting so a heading inside a quote or list item is
        // indented to match its container.
        let indent = self.list_indent().saturating_add(self.quote_indent());
        if indent > 0 {
            p = p.indent(Some(indent), None, None, None);
        }
        self.blocks.push(BlockOut::Para(p));
        self.heading_text.clear();
        self.heading_id = None;
    }

    fn flush_link(&mut self) {
        let Some(link) = self.link.take() else {
            return;
        };
        if link.runs.is_empty() {
            return;
        }
        // In footnote bodies, flatten links to styled runs (appending the URL
        // for external links) so no dangling relationship id is produced.
        if self.flatten_links {
            for r in link.runs {
                self.target_buf()
                    .push(Inline::Run(r.style(styles::HYPERLINK)));
            }
            if !link.anchor && !link.target.is_empty() {
                let note = self.styled_run(&format!(" ({})", link.target), false);
                self.target_buf().push(Inline::Run(note));
            }
            return;
        }
        let kind = if link.anchor {
            HyperlinkType::Anchor
        } else {
            HyperlinkType::External
        };
        let mut h = Hyperlink::new(link.target, kind);
        for r in link.runs {
            h = h.add_run(r.style(styles::HYPERLINK));
        }
        self.target_buf().push(Inline::Link(Box::new(h)));
    }

    fn flush_image(&mut self) {
        let Some(img) = self.image.take() else {
            return;
        };
        match self.load_image(&img) {
            Some(pic) => {
                let r = Run::new().add_image(pic);
                self.emit_run(r);
            }
            None => {
                let label = if img.alt.is_empty() {
                    img.url.clone()
                } else {
                    img.alt.clone()
                };
                // A standalone image (sole content of its paragraph) becomes a
                // captioned placeholder; an inline one stays inline but muted.
                if self.pending.is_empty() && self.link.is_none() && self.table.is_none() {
                    let r = Run::new().add_text(format!("[image: {label}]"));
                    let p = Paragraph::new().style(styles::CAPTION).add_run(r);
                    self.blocks.push(BlockOut::Para(p));
                } else {
                    let r = self
                        .styled_run(&format!("[image: {label}]"), false)
                        .italic()
                        .color(self.opts.caption_color.clone());
                    self.emit_run(r);
                }
            }
        }
    }

    fn flush_code_block(&mut self) {
        let raw = std::mem::take(&mut self.code_buf);
        let body = raw.strip_suffix('\n').unwrap_or(&raw);
        let lines: Vec<&str> = body.split('\n').collect();
        let n = lines.len();

        let mut para = Paragraph::new().style(styles::SOURCE_CODE);
        for (i, line) in lines.iter().enumerate() {
            let r = Run::new()
                .fonts(
                    RunFonts::new()
                        .ascii(self.opts.code_font.clone())
                        .hi_ansi(self.opts.code_font.clone()),
                )
                .size(self.opts.code_half_points())
                .add_text((*line).to_string());
            para = para.add_run(r);
            if i + 1 < n {
                para = para.add_run(Run::new().add_break(BreakType::TextWrapping));
            }
        }

        let indent = self.list_indent().saturating_add(self.quote_indent());
        let cw = (self.opts.page.content_width() as usize).saturating_sub(indent.max(0) as usize);
        let cell = TableCell::new()
            .shading(
                Shading::new()
                    .shd_type(ShdType::Clear)
                    .color("auto")
                    .fill(self.opts.code_fill.clone()),
            )
            .add_paragraph(para);
        // `without_borders` keeps the shaded box but drops the default black grid.
        let mut table = Table::without_borders(vec![TableRow::new(vec![cell])])
            .set_grid(vec![cw])
            .width(cw, WidthType::Dxa)
            .layout(TableLayoutType::Fixed);
        if indent > 0 {
            table = table.indent(indent);
        }
        self.blocks.push(BlockOut::Table(Box::new(table)));
        self.in_code = false;
    }

    fn flush_table_cell(&mut self) {
        let Some(t) = self.table.as_mut() else {
            return;
        };
        let inls = t.cur_cell.take().unwrap_or_default();
        let col = t.col;
        let in_head = t.in_head;
        let align = t.aligns.get(col).copied().unwrap_or(Alignment::None);

        let mut p = Paragraph::new();
        for inl in inls {
            p = match inl {
                Inline::Run(r) => p.add_run(r),
                Inline::Link(h) => p.add_hyperlink(*h),
                Inline::Math(m) => p.add_omath(*m),
            };
        }
        p = match align {
            Alignment::Center => p.align(AlignmentType::Center),
            Alignment::Right => p.align(AlignmentType::Right),
            Alignment::Left => p.align(AlignmentType::Left),
            Alignment::None => p,
        };

        let mut cell = TableCell::new().add_paragraph(p);
        if in_head {
            cell = cell.shading(
                Shading::new()
                    .shd_type(ShdType::Clear)
                    .color("auto")
                    .fill(self.opts.header_fill.clone()),
            );
        }
        t.cur_row.push(cell);
        t.col += 1;
    }

    fn flush_table(&mut self) {
        let Some(t) = self.table.take() else {
            return;
        };
        let indent = self.list_indent().saturating_add(self.quote_indent());
        let ncols = t.aligns.len().max(1);
        let cw = (self.opts.page.content_width() as usize)
            .saturating_sub(indent.max(0) as usize)
            .max(ncols);
        let col_w = cw / ncols;
        let grid = vec![col_w; ncols];

        let mut rows = Vec::with_capacity(t.rows.len());
        for mut cells in t.rows {
            while cells.len() < ncols {
                cells.push(TableCell::new().add_paragraph(Paragraph::new()));
            }
            rows.push(TableRow::new(cells));
        }
        if rows.is_empty() {
            return;
        }
        let mut table = Table::new(rows)
            .set_grid(grid)
            .width(cw, WidthType::Dxa)
            .layout(TableLayoutType::Fixed);
        if indent > 0 {
            table = table.indent(indent);
        }
        self.blocks.push(BlockOut::Table(Box::new(table)));
    }

    fn flush_def(&mut self, kind: DefKind) {
        if self.pending.is_empty() {
            return;
        }
        let mut p = Paragraph::new();
        for inl in std::mem::take(&mut self.pending) {
            p = match inl {
                Inline::Run(r) => p.add_run(r),
                Inline::Link(h) => p.add_hyperlink(*h),
                Inline::Math(m) => p.add_omath(*m),
            };
        }
        p = match kind {
            DefKind::Title => p.bold(),
            DefKind::Definition => p.indent(Some(720), None, None, None),
        };
        self.blocks.push(BlockOut::Para(p));
    }

    // ---- misc builders -----------------------------------------------------

    fn push_alert_label(&mut self, kind: BlockQuoteKind) {
        let (label, color) = match kind {
            BlockQuoteKind::Note => ("Note", "0969DA"),
            BlockQuoteKind::Tip => ("Tip", "1A7F37"),
            BlockQuoteKind::Important => ("Important", "8250DF"),
            BlockQuoteKind::Warning => ("Warning", "9A6700"),
            BlockQuoteKind::Caution => ("Caution", "CF222E"),
        };
        let r = Run::new().add_text(label).bold().color(color);
        let p = Paragraph::new().style(styles::QUOTE).add_run(r);
        self.blocks.push(BlockOut::Para(p));
    }

    /// A thematic break: an empty paragraph carrying a single bottom border,
    /// which Word renders as a horizontal rule spanning the content width. The
    /// rule inherits any list/quote indent so it aligns with its container.
    fn push_hr(&mut self) {
        let border = ParagraphBorder::new(ParagraphBorderPosition::Bottom)
            .val(BorderType::Single)
            .size(6) // eighths of a point → 0.75pt
            .space(1)
            .color("999999");
        let borders = ParagraphBorders::with_empty().set(border);
        let mut p = Paragraph::new().set_borders(borders);
        let indent = self.list_indent().saturating_add(self.quote_indent());
        if indent > 0 {
            p = p.indent(Some(indent), None, None, None);
        }
        self.blocks.push(BlockOut::Para(p));
    }

    /// A run for math source rendered as text (the fallback when native OMML is
    /// disabled, or inside a hyperlink where a `m:oMath` child cannot live):
    /// italic serif (Cambria Math), no code shading, so it reads as math.
    fn math_run(&self, src: &str) -> Run {
        let mut r = Run::new().add_text(src.to_string()).italic().fonts(
            RunFonts::new()
                .ascii("Cambria Math")
                .hi_ansi("Cambria Math"),
        );
        if self.fmt.bold > 0 {
            r = r.bold();
        }
        r
    }

    /// Inline `$…$` math: a native `m:oMath` equation routed into the current
    /// paragraph/cell. Falls back to a text run when native math is disabled or
    /// when inside a hyperlink (which holds runs, not paragraph children).
    fn on_inline_math(&mut self, src: &str) {
        if !self.opts.native_math || self.link.is_some() {
            let r = self.math_run(src);
            self.emit_run(r);
            return;
        }
        let math = crate::math::latex_to_omath(src, false);
        self.target_buf().push(Inline::Math(Box::new(math)));
    }

    fn on_display_math(&mut self, src: &str) {
        // If there is pending inline content (or we're inside a link/cell), the
        // `$$` appeared mid-paragraph: render it inline to preserve flow.
        let inline_context = !self.pending.is_empty()
            || self.link.is_some()
            || self.table.as_ref().is_some_and(|t| t.cur_cell.is_some());

        if !self.opts.native_math {
            if inline_context {
                let r = self.math_run(src);
                self.emit_run(r);
            } else {
                self.flush_text_paragraph();
                let p = Paragraph::new()
                    .align(AlignmentType::Center)
                    .add_run(self.math_run(src));
                // Inherit list/quote styling + indent of the containing block.
                let p = self.decorate(p);
                self.blocks.push(BlockOut::Para(p));
            }
            return;
        }

        if inline_context {
            // Mid-paragraph `$$`: keep it inline (no block break), but inside a
            // hyperlink fall back to text since `m:oMath` is not a run.
            if self.link.is_some() {
                let r = self.math_run(src);
                self.emit_run(r);
            } else {
                let math = crate::math::latex_to_omath(src, false);
                self.target_buf().push(Inline::Math(Box::new(math)));
            }
        } else {
            self.flush_text_paragraph();
            // A display equation: `m:oMathPara` (self-centring) in its own
            // paragraph, inheriting any block-quote/list styling and indent.
            let math = crate::math::latex_to_omath(src, true);
            let p = Paragraph::new().add_omath(math);
            let p = self.decorate(p);
            self.blocks.push(BlockOut::Para(p));
        }
    }

    fn push_footnote_ref(&mut self, label: &CowStr<'_>) {
        let key = label.to_string();
        let paras = self.footnotes.get(&key).cloned();
        if let Some(paras) = paras {
            let mut f = Footnote::new();
            for p in paras {
                f = f.add_content(p);
            }
            let r = Run::new().add_footnote_reference(f);
            self.emit_run(r);
        } else {
            let r = self.styled_run(&format!("[{key}]"), false);
            self.emit_run(r);
        }
    }

    fn on_html_block(&mut self, s: &str) {
        if self.in_metadata {
            return;
        }
        let text = strip_html_tags(s);
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.flush_text_paragraph();
        let r = self.styled_run(text, false);
        let p = Paragraph::new().add_run(r);
        let p = self.decorate(p);
        self.blocks.push(BlockOut::Para(p));
    }

    fn on_inline_html(&mut self, s: &str) {
        let lower = s.trim().to_ascii_lowercase();
        match lower.as_str() {
            "<br>" | "<br/>" | "<br />" => {
                let r = Run::new().add_break(BreakType::TextWrapping);
                self.emit_run(r);
            }
            "<b>" | "<strong>" => self.fmt.bold += 1,
            "</b>" | "</strong>" => self.fmt.bold = self.fmt.bold.saturating_sub(1),
            "<i>" | "<em>" => self.fmt.italic += 1,
            "</i>" | "</em>" => self.fmt.italic = self.fmt.italic.saturating_sub(1),
            "<u>" => self.fmt.underline += 1,
            "</u>" => self.fmt.underline = self.fmt.underline.saturating_sub(1),
            "<s>" | "<del>" | "<strike>" => self.fmt.strike += 1,
            "</s>" | "</del>" | "</strike>" => self.fmt.strike = self.fmt.strike.saturating_sub(1),
            "<code>" | "<kbd>" | "<tt>" => self.fmt.code += 1,
            "</code>" | "</kbd>" | "</tt>" => self.fmt.code = self.fmt.code.saturating_sub(1),
            "<mark>" => self.fmt.highlight += 1,
            "</mark>" => self.fmt.highlight = self.fmt.highlight.saturating_sub(1),
            "<sup>" => self.fmt.superscript += 1,
            "</sup>" => self.fmt.superscript = self.fmt.superscript.saturating_sub(1),
            "<sub>" => self.fmt.subscript += 1,
            "</sub>" => self.fmt.subscript = self.fmt.subscript.saturating_sub(1),
            _ => {}
        }
    }

    // ---- registries --------------------------------------------------------

    /// Allocate a fresh abstract+concrete numbering for one list. Using a fresh
    /// id per list makes ordered lists restart at their start value and lets a
    /// bullet list nest inside an ordered one (and vice-versa); the visual depth
    /// is baked into the single level's indent.
    fn alloc_numbering(&mut self, ordered: bool, level: usize, start: u64) -> usize {
        self.num_counter += 1;
        // docx-rs unconditionally writes a built-in numbering at abstractNumId=1
        // / numId=1, so our ids must start at 2 to avoid a duplicate-id clash
        // that would make Word render the first list with the wrong format.
        let id = self.num_counter + 1;
        let indent_left = ((level as i32) + 1) * 720;
        let lvl = if ordered {
            Level::new(
                0,
                Start::new(start as usize),
                NumberFormat::new("decimal"),
                LevelText::new("%1."),
                LevelJc::new("left"),
            )
        } else {
            Level::new(
                0,
                Start::new(1),
                NumberFormat::new("bullet"),
                LevelText::new(bullet_glyph(level)),
                LevelJc::new("left"),
            )
        }
        .indent(
            Some(indent_left),
            Some(SpecialIndentType::Hanging(360)),
            None,
            None,
        );
        self.abstracts
            .push(AbstractNumbering::new(id).add_level(lvl));
        self.numberings.push(Numbering::new(id, id));
        id
    }

    fn next_bookmark(&mut self) -> usize {
        self.bookmark_counter += 1;
        self.bookmark_counter
    }

    fn unique_anchor(&mut self, base: String) -> String {
        if self.used_anchors.insert(base.clone()) {
            return base;
        }
        let mut i = 1;
        loop {
            let cand = format!("{base}-{i}");
            if self.used_anchors.insert(cand.clone()) {
                return cand;
            }
            i += 1;
        }
    }

    // ---- images ------------------------------------------------------------

    /// Load an image from a local path or `data:` URI and build a ready-to-embed
    /// `Pic`, sized to fit the content box. We decode and re-encode to PNG here
    /// (rather than via `Pic::new`, whose internal `.expect()`s panic on decode
    /// *or* encode failure) and build the `Pic` with `new_with_dimensions`, which
    /// does no decoding. Remote URLs are not fetched. Returns `None` on any
    /// failure (caller falls back to alt text).
    fn load_image(&self, img: &ImageCtx) -> Option<Pic> {
        let bytes = if let Some(rest) = img.url.strip_prefix("data:") {
            let comma = rest.find(',')?;
            let meta = &rest[..comma];
            let data = &rest[comma + 1..];
            if !meta.contains("base64") {
                return None;
            }
            base64::engine::general_purpose::STANDARD
                .decode(data)
                .ok()?
        } else if img.url.starts_with("http://") || img.url.starts_with("https://") {
            return None;
        } else {
            std::fs::read(self.resolve_path(&img.url)).ok()?
        };

        let decoded = image::load_from_memory(&bytes).ok()?;
        let (w, h) = decoded.dimensions();
        // Reject degenerate / absurd dimensions (the latter would overflow
        // docx-rs's `from_px` = px * 9525 inside `new_with_dimensions`).
        if w == 0 || h == 0 || w > 60_000 || h > 60_000 {
            return None;
        }

        // Re-encode to PNG ourselves; bail to alt-text on encode failure.
        let mut png = std::io::Cursor::new(Vec::new());
        decoded.write_to(&mut png, image::ImageFormat::Png).ok()?;
        let png_bytes = png.into_inner();

        // Cap to the printable content box (1 twip = 635 EMU, 1 px = 9525 EMU).
        let content_w = self.opts.page.content_width() as u64 * 635;
        let content_h = self.opts.page.content_height() as u64 * 635;
        let mut w_emu = w as u64 * 9525;
        let mut h_emu = h as u64 * 9525;
        if w_emu > content_w {
            let ratio = content_w as f64 / w_emu as f64;
            w_emu = content_w;
            h_emu = (h_emu as f64 * ratio).round() as u64;
        }
        if h_emu > content_h {
            let ratio = content_h as f64 / h_emu as f64;
            h_emu = content_h;
            w_emu = (w_emu as f64 * ratio).round() as u64;
        }

        Some(Pic::new_with_dimensions(png_bytes, w, h).size(w_emu as u32, h_emu as u32))
    }

    fn resolve_path(&self, url: &str) -> PathBuf {
        let decoded = url.replace("%20", " ");
        let p = Path::new(&decoded);
        if p.is_absolute() {
            return p.to_path_buf();
        }
        match &self.opts.base_dir {
            Some(base) => base.join(p),
            None => p.to_path_buf(),
        }
    }
}

// ---- free helpers ----------------------------------------------------------

fn checkbox_run(checked: bool) -> Run {
    let glyph = if checked { "\u{2612} " } else { "\u{2610} " };
    Run::new().add_text(glyph)
}

/// If a `^…^` / `~…~` script span opens at `open` (delimiter byte `delim`),
/// return the byte index of its closing delimiter. A span is valid only when it
/// is a *single* delimiter (not `^^`/`~~`), has non-empty content, and contains
/// no whitespace — mirroring the intraword, paired-delimiter convention. The
/// returned index is a valid char boundary because the delimiters are ASCII.
fn script_span(t: &str, open: usize, delim: u8) -> Option<usize> {
    let bytes = t.as_bytes();
    // `~~` is strikethrough (and `^^` is meaningless); never a script.
    if bytes.get(open + 1) == Some(&delim) {
        return None;
    }
    let delim_ch = delim as char;
    for (off, ch) in t[open + 1..].char_indices() {
        let j = open + 1 + off;
        if ch == delim_ch {
            return if j > open + 1 { Some(j) } else { None };
        }
        // Any whitespace — including Unicode spaces such as U+00A0/U+2009 —
        // terminates the candidate span.
        if ch.is_whitespace() {
            return None;
        }
    }
    None
}

fn bullet_glyph(level: usize) -> &'static str {
    match level % 3 {
        0 => "\u{2022}", // •
        1 => "\u{25E6}", // ◦
        _ => "\u{25AA}", // ▪
    }
}

/// GitHub-style heading slug: lowercase, alphanumerics kept, spaces/underscores
/// to hyphens, other punctuation dropped, consecutive hyphens collapsed.
fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in s.chars() {
        if ch.is_alphanumeric() {
            for c in ch.to_lowercase() {
                out.push(c);
            }
            prev_dash = false;
        } else if (ch == ' ' || ch == '-' || ch == '_') && !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Sanitise a slug into a Word-legal bookmark name (must start with a letter or
/// underscore, restricted charset, <= 40 chars).
fn bookmark_name(slug: &str) -> String {
    let mut n: String = slug
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if n.is_empty() {
        n = "_anchor".to_string();
    }
    let first = n.chars().next().unwrap();
    if !first.is_alphabetic() && first != '_' {
        n = format!("_{n}");
    }
    if n.chars().count() > 40 {
        n = n.chars().take(40).collect();
    }
    n
}

/// Turn a bare email (or scheme-less link that looks like one) into a `mailto:`.
fn normalize_link(url: &str) -> String {
    if url.starts_with("mailto:") || url.contains("://") {
        return url.to_string();
    }
    if url.contains('@') && !url.contains('/') {
        return format!("mailto:{url}");
    }
    url.to_string()
}

/// Crude tag stripper for best-effort rendering of raw HTML blocks.
fn strip_html_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}
