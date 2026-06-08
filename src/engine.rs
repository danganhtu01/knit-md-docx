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

        match ev {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(t) => self.on_text(&t),
            Event::Code(t) => {
                let r = self.styled_run(&t, true);
                self.emit_run(r);
            }
            Event::InlineMath(t) => {
                let r = self.styled_run(&t, true);
                self.emit_run(r);
            }
            Event::DisplayMath(t) => self.push_display_math(&t),
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
            // Superscript/Subscript extensions are not enabled; render inline.
            Tag::Superscript | Tag::Subscript => {}
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
            TagEnd::Superscript | TagEnd::Subscript => {}
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
        let r = self.styled_run(t, false);
        self.emit_run(r);
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
                        .fill("EEEEEE"),
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
            };
        }
        p = self.decorate(p);
        self.blocks.push(BlockOut::Para(p));
    }

    /// Apply list numbering / block-quote styling to a body paragraph.
    fn decorate(&mut self, mut p: Paragraph) -> Paragraph {
        if let Some(list) = self.list_stack.last().copied() {
            if let Some(item) = self.item_stack.last_mut() {
                let cont_indent = ((list.level as i32) + 1) * 720;
                if item.task.is_some() {
                    p = p.indent(Some(cont_indent), None, None, None);
                } else if !item.numbered {
                    p = p.numbering(NumberingId::new(list.num_id), IndentLevel::new(0));
                    item.numbered = true;
                } else {
                    p = p.indent(Some(cont_indent), None, None, None);
                }
            }
        }
        if self.quote_depth > 0 {
            p = p.style(styles::QUOTE);
            if self.quote_depth > 1 {
                p = p.indent(Some(self.quote_depth as i32 * 360 + 360), None, None, None);
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
            let raw = self
                .heading_id
                .take()
                .filter(|s| !s.is_empty())
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
            };
        }
        if let Some(id) = bid {
            p = p.add_bookmark_end(id);
        }
        p = p.style(style);
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
            Some((bytes, w_emu, h_emu)) => {
                let pic = Pic::new(&bytes).size(w_emu, h_emu);
                let r = Run::new().add_image(pic);
                self.emit_run(r);
            }
            None => {
                let label = if img.alt.is_empty() {
                    img.url.clone()
                } else {
                    img.alt.clone()
                };
                let r = self.styled_run(&format!("[image: {label}]"), false);
                self.emit_run(r);
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
                .size(20)
                .add_text((*line).to_string());
            para = para.add_run(r);
            if i + 1 < n {
                para = para.add_run(Run::new().add_break(BreakType::TextWrapping));
            }
        }

        let cw = self.opts.page.content_width() as usize;
        let cell = TableCell::new()
            .shading(
                Shading::new()
                    .shd_type(ShdType::Clear)
                    .color("auto")
                    .fill(styles::CODE_FILL),
            )
            .add_paragraph(para);
        let table = Table::new(vec![TableRow::new(vec![cell])])
            .set_grid(vec![cw])
            .width(cw, WidthType::Dxa)
            .layout(TableLayoutType::Fixed);
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
                    .fill(styles::HEADER_FILL),
            );
        }
        t.cur_row.push(cell);
        t.col += 1;
    }

    fn flush_table(&mut self) {
        let Some(t) = self.table.take() else {
            return;
        };
        let ncols = t.aligns.len().max(1);
        let cw = (self.opts.page.content_width() as usize).max(ncols);
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
        let table = Table::new(rows)
            .set_grid(grid)
            .width(cw, WidthType::Dxa)
            .layout(TableLayoutType::Fixed);
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

    fn push_hr(&mut self) {
        let r = Run::new().add_text("\u{2014}".repeat(40)).color("BFBFBF");
        let p = Paragraph::new().align(AlignmentType::Center).add_run(r);
        self.blocks.push(BlockOut::Para(p));
    }

    fn push_display_math(&mut self, src: &str) {
        self.flush_text_paragraph();
        let r = Run::new()
            .fonts(
                RunFonts::new()
                    .ascii(self.opts.code_font.clone())
                    .hi_ansi(self.opts.code_font.clone()),
            )
            .size(20)
            .add_text(src.to_string());
        let p = Paragraph::new().align(AlignmentType::Center).add_run(r);
        self.blocks.push(BlockOut::Para(p));
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
        let id = self.num_counter;
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

    /// Load image bytes from a local path or `data:` URI, validate them with the
    /// `image` crate (so we never feed undecodable bytes to `Pic::new`, which
    /// panics), and compute a size capped at the content width. Remote URLs are
    /// not fetched. Returns `None` on any failure (caller falls back to alt text).
    fn load_image(&self, img: &ImageCtx) -> Option<(Vec<u8>, u32, u32)> {
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
        if w == 0 || h == 0 {
            return None;
        }

        let content_emu = self.opts.page.content_width() as u64 * 635;
        let mut w_emu = w as u64 * 9525;
        let mut h_emu = h as u64 * 9525;
        if w_emu > content_emu {
            let ratio = content_emu as f64 / w_emu as f64;
            w_emu = content_emu;
            h_emu = (h_emu as f64 * ratio).round() as u64;
        }
        Some((bytes, w_emu as u32, h_emu as u32))
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
