//! Markdown/HTML parsing is kept separate from terminal layout and escape emission.
use comrak::{markdown_to_html, Options};
use scraper::{ElementRef, Html, Node};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style(u8);
impl Style {
    const BOLD: Self = Self(1);
    const ITALIC: Self = Self(2);
    const UNDERLINE: Self = Self(4);
    const DIM: Self = Self(8);
    const REVERSE: Self = Self(16);
    const STRIKE: Self = Self(32);
    const CODE: Self = Self(64);
    fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
    fn ansi(self) -> String {
        let mut codes = vec!["0"];
        for (flag, code) in [
            (1, "1"),
            (2, "3"),
            (4, "4"),
            (8, "2"),
            (16, "7"),
            (32, "9"),
            (64, "7"),
        ] {
            if self.0 & flag != 0 {
                codes.push(code);
            }
        }
        format!("\x1b[{}m", codes.join(";"))
    }
}

#[derive(Clone, Debug)]
struct Span {
    text: String,
    style: Style,
}
#[derive(Clone, Debug)]
enum Inline {
    Text(Span),
    Break,
}
#[derive(Clone, Debug, Default)]
pub struct Line {
    spans: Vec<Span>,
}
impl Line {
    fn push(&mut self, text: &str, style: Style) {
        if text.is_empty() {
            return;
        }
        let safe = clean(text, false);
        let text = safe.as_str();
        if let Some(last) = self.spans.last_mut().filter(|s| s.style == style) {
            last.text.push_str(text);
        } else {
            self.spans.push(Span {
                text: text.to_owned(),
                style,
            });
        }
    }
    fn append(&mut self, other: &Line) {
        for span in &other.spans {
            self.push(&span.text, span.style);
        }
    }
    pub fn width(&self) -> usize {
        self.spans.iter().map(|s| display_width(&s.text)).sum()
    }
    fn text(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }
    fn styled(text: &str, style: Style) -> Self {
        let mut line = Self::default();
        line.push(text, style);
        line
    }
    /// Emit only our own SGR sequences, never escapes supplied by a document.
    pub fn ansi(&self, width: usize) -> String {
        let mut out = String::from("\x1b[0m");
        let mut used = 0;
        'spans: for span in &self.spans {
            out.push_str(&span.style.ansi());
            for g in span.text.graphemes(true) {
                let cells = display_width(g);
                if used + cells > width {
                    break 'spans;
                }
                out.push_str(g);
                used += cells;
            }
        }
        out.push_str("\x1b[0m");
        out.push_str(&" ".repeat(width.saturating_sub(used)));
        out
    }
}

pub fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}
fn clean(text: &str, literal: bool) -> String {
    text.chars()
        .map(|c| match c {
            '\n' | '\t' if literal => c,
            '\n' | '\r' | '\t' => ' ',
            c if c.is_control() => '\u{fffd}',
            c => c,
        })
        .collect()
}
fn inline(text: &str, style: Style) -> Inline {
    Inline::Text(Span {
        text: clean(text, false),
        style,
    })
}

#[derive(Clone, Debug)]
enum Block {
    Paragraph(Vec<Inline>),
    Heading(u8, Vec<Inline>),
    Quote(Vec<Block>),
    Indent(Vec<Block>),
    List {
        items: Vec<(String, Vec<Block>)>,
        loose: bool,
    },
    Pre {
        language: String,
        text: String,
    },
    Literal(String),
    Table(Vec<Vec<Cell>>),
    Rule,
}
#[derive(Clone, Debug, Default)]
struct Cell {
    blocks: Vec<Block>,
    heading: bool,
    align: Align,
}
#[derive(Clone, Copy, Debug, Default)]
enum Align {
    #[default]
    Left,
    Center,
    Right,
}

// An owned tree makes the Markdown and HTML paths use the same block/inline rules.
#[derive(Debug)]
struct Element {
    tag: String,
    attrs: Vec<(String, String)>,
    children: Vec<Content>,
}
#[derive(Debug)]
enum Content {
    Text(String),
    Element(Element),
}
impl Element {
    fn read(element: ElementRef<'_>) -> Self {
        let children = element
            .children()
            .filter_map(|child| match child.value() {
                Node::Text(text) => Some(Content::Text(text.to_string())),
                Node::Element(_) => {
                    ElementRef::wrap(child).map(|e| Content::Element(Self::read(e)))
                }
                _ => None,
            })
            .collect();
        Self {
            tag: element.value().name().to_owned(),
            attrs: element
                .value()
                .attrs()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
            children,
        }
    }
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
    fn has_class(&self, name: &str) -> bool {
        self.attr("class")
            .unwrap_or("")
            .split_whitespace()
            .any(|c| c == name)
    }
    fn text(&self) -> String {
        self.children
            .iter()
            .map(|c| match c {
                Content::Text(t) => t.clone(),
                Content::Element(e) => e.text(),
            })
            .collect()
    }
    fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|c| match c {
            Content::Element(e) => Some(e),
            _ => None,
        })
    }
}

#[derive(Default)]
struct Document {
    blocks: Vec<Block>,
}
#[derive(Default)]
struct Builder {
    links: Vec<String>,
}
impl Builder {
    fn link(&mut self, url: &str) -> String {
        let url = clean(url, false);
        if url.is_empty() {
            return String::new();
        }
        let index = if let Some(i) = self.links.iter().position(|u| u == &url) {
            i
        } else {
            self.links.push(url);
            self.links.len() - 1
        };
        format!("[{}]", index + 1)
    }
    fn inlines(&mut self, nodes: &[Content], style: Style) -> Vec<Inline> {
        let mut out = Vec::new();
        for node in nodes {
            match node {
                Content::Text(text) => out.push(inline(text, style)),
                Content::Element(e) => {
                    if ignored(&e.tag) {
                        continue;
                    }
                    let mut nested_style = style;
                    match e.tag.as_str() {
                        "strong" | "b" => nested_style = style.with(Style::BOLD),
                        "em" | "i" => nested_style = style.with(Style::ITALIC),
                        "u" | "ins" => nested_style = style.with(Style::UNDERLINE),
                        "del" | "s" | "strike" => nested_style = style.with(Style::STRIKE),
                        "mark" => nested_style = style.with(Style::REVERSE),
                        "code" | "kbd" | "samp" => nested_style = style.with(Style::CODE),
                        "small" => nested_style = style.with(Style::DIM),
                        _ => {}
                    }
                    if e.has_class("spoiler") {
                        nested_style = nested_style.with(Style::DIM);
                    }
                    if let Some(math) = e.attr("data-math-style") {
                        let delimiter = if math == "display" { "$$" } else { "$" };
                        out.push(inline(
                            &format!("{delimiter}{}{delimiter}", e.text()),
                            style.with(Style::ITALIC),
                        ));
                        continue;
                    }
                    match e.tag.as_str() {
                        "br" => out.push(Inline::Break),
                        "a" if e.attr("data-footnote-backref").is_some() => {}
                        "a" if e.attr("data-footnote-ref").is_some() => {
                            out.push(inline(&format!("[^{}]", e.text()), style.with(Style::DIM)))
                        }
                        "a" => {
                            let label = self.inlines(&e.children, style.with(Style::UNDERLINE));
                            if label.is_empty() {
                                out.push(inline(e.attr("href").unwrap_or(""), style));
                            } else {
                                out.extend(label);
                            }
                            let reference = self.link(e.attr("href").unwrap_or(""));
                            out.push(inline(&reference, style.with(Style::DIM)));
                        }
                        "img" => {
                            let alt = e.attr("alt").filter(|s| !s.is_empty()).unwrap_or("image");
                            let reference = self.link(e.attr("src").unwrap_or(""));
                            out.push(inline(
                                &format!("[Image: {alt}]{reference}"),
                                style.with(Style::DIM),
                            ));
                        }
                        "input" if e.attr("type") == Some("checkbox") => {
                            out.push(inline(
                                if e.attr("checked").is_some() {
                                    "[x] "
                                } else {
                                    "[ ] "
                                },
                                style,
                            ));
                        }
                        "sup" if e.elements().any(|c| c.attr("data-footnote-ref").is_some()) => {
                            out.extend(self.inlines(&e.children, style))
                        }
                        "sup" | "sub" => {
                            out.push(inline(&script_text(&e.text(), e.tag == "sup"), style))
                        }
                        _ => out.extend(self.inlines(&e.children, nested_style)),
                    }
                }
            }
        }
        out
    }
    fn blocks(&mut self, nodes: &[Content], style: Style) -> Vec<Block> {
        let mut out = Vec::new();
        let mut pending = Vec::new();
        for node in nodes {
            let Content::Element(e) = node else {
                pending.extend(self.inlines(std::slice::from_ref(node), style));
                continue;
            };
            if ignored(&e.tag) {
                continue;
            }
            if !is_block(&e.tag)
                && !e.elements().any(contains_block)
                && e.attr("data-math-style") != Some("display")
            {
                pending.extend(self.inlines(std::slice::from_ref(node), style));
                continue;
            }
            flush_paragraph(&mut pending, &mut out);
            if e.attr("data-math-style") == Some("display") {
                out.push(Block::Pre {
                    language: "math".into(),
                    text: format!("$$\n{}\n$$", clean(e.text().trim_matches('\n'), true)),
                });
                continue;
            }
            match e.tag.as_str() {
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    out.push(Block::Heading(
                        e.tag.as_bytes()[1] - b'0',
                        self.inlines(&e.children, style),
                    ));
                }
                "hr" => out.push(Block::Rule),
                "pre" => {
                    let code = e.elements().find(|c| c.tag == "code");
                    let language = code
                        .and_then(|c| c.attr("class"))
                        .unwrap_or("")
                        .split_whitespace()
                        .find_map(|c| c.strip_prefix("language-"))
                        .unwrap_or("");
                    out.push(Block::Pre {
                        language: language.to_owned(),
                        text: clean(&e.text(), true).trim_end_matches('\n').to_owned(),
                    });
                }
                "blockquote" => out.push(Block::Quote(self.blocks(&e.children, style))),
                "div" if e.has_class("markdown-alert") => {
                    out.push(Block::Quote(self.blocks(&e.children, style)))
                }
                "ul" | "ol" => out.push(self.list(e, style, false)),
                "section" if e.attr("data-footnotes").is_some() => {
                    out.push(Block::Heading(2, vec![inline("Footnotes", style)]));
                    for child in e.elements() {
                        if child.tag == "ol" {
                            out.push(self.list(child, style, true));
                        } else {
                            out.extend(self.blocks(&child.children, style));
                        }
                    }
                }
                "table" => {
                    for caption in e.elements().filter(|c| c.tag == "caption") {
                        out.push(Block::Paragraph(
                            self.inlines(&caption.children, style.with(Style::BOLD)),
                        ));
                    }
                    let mut rows = Vec::new();
                    self.table_rows(e, style, &mut rows);
                    if !rows.is_empty() {
                        out.push(Block::Table(rows));
                    }
                }
                "dt" | "summary" => out.push(Block::Paragraph(
                    self.inlines(&e.children, style.with(Style::BOLD)),
                )),
                "dd" => out.push(Block::Indent(self.blocks(&e.children, style))),
                "p" => {
                    let title_style = if e.has_class("markdown-alert-title") {
                        style.with(Style::BOLD)
                    } else {
                        style
                    };
                    if e.elements()
                        .any(|child| child.attr("data-math-style") == Some("display"))
                    {
                        out.extend(self.blocks(&e.children, title_style));
                    } else {
                        let mut content = self.inlines(&e.children, title_style);
                        flush_paragraph(&mut content, &mut out);
                    }
                }
                _ => out.extend(self.blocks(&e.children, style)),
            }
        }
        flush_paragraph(&mut pending, &mut out);
        out
    }
    fn list(&mut self, element: &Element, style: Style, footnotes: bool) -> Block {
        let mut number = element
            .attr("start")
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(1);
        let mut items = Vec::new();
        let mut loose = false;
        for item in element.elements().filter(|e| e.tag == "li") {
            if let Some(value) = item.attr("value").and_then(|s| s.parse().ok()) {
                number = value;
            }
            let marker = if footnotes {
                format!("[^{number}] ")
            } else if element.tag == "ol" {
                format!("{number}. ")
            } else {
                "• ".into()
            };
            loose |= item.elements().any(|e| e.tag == "p");
            items.push((marker, self.blocks(&item.children, style)));
            number = number.saturating_add(1);
        }
        Block::List { items, loose }
    }
    fn table_rows(&mut self, element: &Element, style: Style, rows: &mut Vec<Vec<Cell>>) {
        for child in element.elements() {
            if child.tag == "tr" {
                let mut cells = Vec::new();
                for cell in child.elements().filter(|e| e.tag == "td" || e.tag == "th") {
                    let align = match cell.attr("align").unwrap_or("") {
                        "right" => Align::Right,
                        "center" => Align::Center,
                        _ => Align::Left,
                    };
                    cells.push(Cell {
                        blocks: self.blocks(&cell.children, style),
                        heading: cell.tag == "th",
                        align,
                    });
                    let colspan = cell
                        .attr("colspan")
                        .and_then(|s| s.parse::<usize>().ok())
                        .unwrap_or(1)
                        .clamp(1, 128);
                    cells.extend((1..colspan).map(|_| Cell::default()));
                }
                if !cells.is_empty() {
                    rows.push(cells);
                }
            } else if matches!(child.tag.as_str(), "thead" | "tbody" | "tfoot") {
                self.table_rows(child, style, rows);
            }
        }
    }
}
fn contains_block(e: &Element) -> bool {
    is_block(&e.tag)
        || e.attr("data-math-style") == Some("display")
        || e.elements().any(contains_block)
}
fn ignored(tag: &str) -> bool {
    matches!(
        tag,
        "script" | "style" | "head" | "template" | "iframe" | "object" | "embed" | "noscript"
    )
}
fn is_block(tag: &str) -> bool {
    matches!(
        tag,
        "html"
            | "body"
            | "div"
            | "section"
            | "article"
            | "main"
            | "header"
            | "footer"
            | "aside"
            | "nav"
            | "p"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "pre"
            | "blockquote"
            | "ul"
            | "ol"
            | "li"
            | "table"
            | "hr"
            | "dl"
            | "dt"
            | "dd"
            | "details"
            | "summary"
            | "figure"
            | "figcaption"
            | "address"
            | "center"
    )
}
fn flush_paragraph(pending: &mut Vec<Inline>, out: &mut Vec<Block>) {
    if pending.iter().any(|i| match i {
        Inline::Break => true,
        Inline::Text(s) => !s.text.trim().is_empty(),
    }) {
        out.push(Block::Paragraph(std::mem::take(pending)));
    } else {
        pending.clear();
    }
}
fn script_text(text: &str, sup: bool) -> String {
    let normal = "0123456789+-=()in";
    let mapped = if sup {
        "⁰¹²³⁴⁵⁶⁷⁸⁹⁺⁻⁼⁽⁾ⁱⁿ"
    } else {
        "₀₁₂₃₄₅₆₇₈₉₊₋₌₍₎ᵢₙ"
    };
    let result: Option<String> = text
        .chars()
        .map(|c| {
            normal
                .chars()
                .position(|n| n == c)
                .and_then(|i| mapped.chars().nth(i))
        })
        .collect();
    result.unwrap_or_else(|| format!("{}({text})", if sup { '^' } else { '_' }))
}
fn front_matter(source: &str) -> (Option<Block>, &str) {
    let Some((first, _)) = source.split_once('\n') else {
        return (None, source);
    };
    if !matches!(first.trim_end_matches('\r'), "---" | "+++") {
        return (None, source);
    }
    let mut offset = first.len() + 1;
    for line in source[offset..].split_inclusive('\n') {
        offset += line.len();
        if line.trim_end_matches(['\r', '\n']) == first.trim_end_matches('\r') {
            return (
                Some(Block::Pre {
                    language: if first.starts_with('-') {
                        "metadata (yaml)"
                    } else {
                        "metadata (toml)"
                    }
                    .into(),
                    text: clean(source[..offset].trim_end_matches('\n'), true),
                }),
                &source[offset..],
            );
        }
    }
    (None, source)
}
impl Document {
    fn parse(source: &str, markdown: bool) -> Self {
        if !markdown {
            return Self {
                blocks: vec![Block::Literal(clean(source, true))],
            };
        }
        let (metadata, source) = front_matter(source);
        let mut options = Options::default();
        let ext = &mut options.extension;
        ext.strikethrough = true;
        ext.table = true;
        ext.autolink = true;
        ext.tasklist = true;
        ext.footnotes = true;
        ext.inline_footnotes = true;
        ext.description_lists = true;
        ext.superscript = true;
        ext.subscript = true;
        ext.highlight = true;
        ext.insert = true;
        ext.spoiler = true;
        ext.shortcodes = true;
        ext.wikilinks_title_after_pipe = true;
        ext.multiline_block_quotes = true;
        ext.alerts = true;
        ext.math_dollars = true;
        ext.math_latex = true;
        ext.math_code = true;
        // HTML is parsed locally as data. Nothing is executed, fetched, or passed to a browser.
        options.render.r#unsafe = true;
        let html = markdown_to_html(source, &options);
        let dom = Html::parse_document(&html);
        let root = Element::read(dom.root_element());
        let mut builder = Builder::default();
        let mut blocks = metadata.into_iter().collect::<Vec<_>>();
        blocks.extend(builder.blocks(&root.children, Style::default()));
        if !builder.links.is_empty() {
            blocks.push(Block::Heading(2, vec![inline("Links", Style::default())]));
            blocks.push(Block::List {
                items: builder
                    .links
                    .iter()
                    .enumerate()
                    .map(|(i, url)| {
                        (
                            format!("[{}] ", i + 1),
                            vec![Block::Paragraph(vec![inline(url, Style::UNDERLINE)])],
                        )
                    })
                    .collect(),
                loose: false,
            });
        }
        Self { blocks }
    }
}

#[derive(Default)]
pub struct PreviewCache {
    revision: Option<u64>,
    markdown: bool,
    document: Document,
    width: usize,
    lines: Vec<Line>,
}
impl PreviewCache {
    pub fn lines(
        &mut self,
        source: &[String],
        revision: u64,
        markdown: bool,
        width: usize,
    ) -> &[Line] {
        if self.revision != Some(revision) || self.markdown != markdown {
            self.document = Document::parse(&source.join("\n"), markdown);
            self.revision = Some(revision);
            self.markdown = markdown;
            self.width = 0;
        }
        let width = width.max(1);
        if self.width != width {
            self.lines = layout(&self.document.blocks, width);
            self.width = width;
        }
        &self.lines
    }
}

#[derive(Clone)]
struct Glyph {
    text: String,
    style: Style,
}
fn word_into_line(
    word: &mut Vec<Glyph>,
    space: &mut bool,
    line: &mut Line,
    out: &mut Vec<Line>,
    width: usize,
) {
    if word.is_empty() {
        return;
    }
    let word_width: usize = word.iter().map(|g| display_width(&g.text)).sum();
    let needs_space = *space && !line.spans.is_empty();
    if !line.spans.is_empty() && line.width() + usize::from(needs_space) + word_width > width {
        out.push(std::mem::take(line));
    } else if needs_space {
        line.push(" ", Style::default());
    }
    for glyph in word.drain(..) {
        let cells = display_width(&glyph.text);
        if line.width() + cells > width && !line.spans.is_empty() {
            out.push(std::mem::take(line));
        }
        // A two-cell grapheme cannot fit into a one-cell viewport, even on an empty line.
        line.push(if cells > width { "�" } else { &glyph.text }, glyph.style);
    }
    *space = false;
}
fn wrap_inline(inlines: &[Inline], width: usize) -> Vec<Line> {
    let width = width.max(1);
    let mut out = Vec::new();
    let mut line = Line::default();
    let mut word = Vec::new();
    let mut space = false;
    for item in inlines {
        match item {
            Inline::Break => {
                word_into_line(&mut word, &mut space, &mut line, &mut out, width);
                out.push(std::mem::take(&mut line));
                space = false;
            }
            Inline::Text(span) => {
                for g in span.text.graphemes(true) {
                    if span.style.0 & Style::CODE.0 == 0
                        && g.chars().all(|c| c.is_whitespace() && c != '\u{a0}')
                    {
                        word_into_line(&mut word, &mut space, &mut line, &mut out, width);
                        space = true;
                    } else {
                        word.push(Glyph {
                            text: g.to_owned(),
                            style: span.style,
                        });
                    }
                }
            }
        }
    }
    word_into_line(&mut word, &mut space, &mut line, &mut out, width);
    if !line.spans.is_empty() || out.is_empty() {
        out.push(line);
    }
    out
}
fn prefix_text(text: &str, width: usize) -> String {
    let mut result = String::new();
    let mut cells = 0;
    for g in text.graphemes(true) {
        if cells + display_width(g) > width {
            break;
        }
        result.push_str(g);
        cells += display_width(g);
    }
    result
}
fn prefixed(blocks: &[Block], width: usize, first: &str, rest: &str) -> Vec<Line> {
    let first = prefix_text(first, width.saturating_sub(1));
    let rest = prefix_text(rest, width.saturating_sub(1));
    let prefix_width = display_width(&first).max(display_width(&rest));
    layout(blocks, width.saturating_sub(prefix_width).max(1))
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            let mut result = Line::styled(if i == 0 { &first } else { &rest }, Style::DIM);
            result.append(&line);
            result
        })
        .collect()
}
fn literal_lines(text: &str, width: usize, code: bool) -> Vec<Line> {
    let mut out = Vec::new();
    let prefix = if code && width >= 4 { "│ " } else { "" };
    let inner = width.saturating_sub(display_width(prefix)).max(1);
    for source_line in text.split('\n') {
        let mut expanded = String::new();
        let mut position = 0;
        for g in source_line.graphemes(true) {
            if g == "\t" {
                let n = 4 - position % 4;
                expanded.push_str(&" ".repeat(n));
                position += n;
            } else {
                expanded.push_str(g);
                position += display_width(g);
            }
        }
        let mut line = Line::styled(prefix, Style::DIM);
        let mut used = 0;
        let mut limit = inner;
        for g in expanded.graphemes(true) {
            let cells = display_width(g);
            if used + cells > limit && used > 0 {
                out.push(line);
                line = Line::styled(prefix, Style::DIM);
                if code && inner >= 3 {
                    line.push("↪ ", Style::DIM);
                    limit = inner - 2;
                }
                used = 0;
            }
            line.push(if cells > limit { "�" } else { g }, Style::default());
            used += cells.min(limit);
        }
        out.push(line);
    }
    out
}
fn heading_style(level: u8) -> Style {
    match level {
        1 => Style::BOLD.with(Style::UNDERLINE),
        2 => Style::BOLD,
        3 => Style::BOLD.with(Style::ITALIC),
        4 => Style::UNDERLINE,
        5 => Style::ITALIC,
        _ => Style::DIM.with(Style::ITALIC),
    }
}
fn layout(blocks: &[Block], width: usize) -> Vec<Line> {
    let width = width.max(1);
    let mut out = Vec::new();
    for block in blocks {
        if !out.is_empty() {
            out.push(Line::default());
        }
        let lines = match block {
            Block::Paragraph(content) => wrap_inline(content, width),
            Block::Heading(level, content) => {
                let styled: Vec<_> = content
                    .iter()
                    .map(|i| match i {
                        Inline::Text(s) => Inline::Text(Span {
                            text: s.text.clone(),
                            style: s.style.with(heading_style(*level)),
                        }),
                        Inline::Break => Inline::Break,
                    })
                    .collect();
                wrap_inline(&styled, width)
            }
            Block::Quote(blocks) => prefixed(blocks, width, "│ ", "│ "),
            Block::Indent(blocks) => prefixed(blocks, width, "  ", "  "),
            Block::Rule => vec![Line::styled(&"─".repeat(width), Style::DIM)],
            Block::Literal(text) => literal_lines(text, width, false),
            Block::Pre { language, text } => {
                let mut lines = Vec::new();
                if !language.is_empty() {
                    lines.extend(wrap_inline(&[inline(language, Style::DIM)], width));
                }
                lines.extend(literal_lines(text, width, true));
                lines
            }
            Block::List { items, loose } => {
                let mut lines = Vec::new();
                for (marker, blocks) in items {
                    if *loose && !lines.is_empty() {
                        lines.push(Line::default());
                    }
                    lines.extend(prefixed(
                        blocks,
                        width,
                        marker,
                        &" ".repeat(display_width(marker)),
                    ));
                }
                lines
            }
            Block::Table(rows) => layout_table(rows, width),
        };
        out.extend(lines);
    }
    if out.is_empty() {
        out.push(Line::default());
    }
    out
}
fn table_border(widths: &[usize], left: char, join: char, right: char) -> Line {
    let parts: Vec<String> = widths.iter().map(|w| "─".repeat(w + 2)).collect();
    Line::styled(
        &format!("{left}{}{right}", parts.join(&join.to_string())),
        Style::DIM,
    )
}
fn layout_table(rows: &[Vec<Cell>], width: usize) -> Vec<Line> {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    if columns == 0 {
        return vec![];
    }
    // Below four cells of text per column, a labelled record view is more useful.
    let overhead = columns * 3 + 1;
    if width < overhead + 4 * columns {
        return stacked_table(rows, width, columns);
    }
    let mut desired = vec![4; columns];
    for row in rows {
        for (column, cell) in row.iter().enumerate() {
            desired[column] = desired[column].max(
                layout(&cell.blocks, width)
                    .iter()
                    .map(Line::width)
                    .max()
                    .unwrap_or(0),
            );
        }
    }
    let mut widths = vec![4; columns];
    let mut remaining = width - overhead - columns * 4;
    while remaining > 0 {
        let mut grew = false;
        for col in 0..columns {
            if remaining > 0 && widths[col] < desired[col] {
                widths[col] += 1;
                remaining -= 1;
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    let mut out = vec![table_border(&widths, '┌', '┬', '┐')];
    for (row_index, row) in rows.iter().enumerate() {
        let contents: Vec<Vec<Line>> = (0..columns)
            .map(|i| {
                row.get(i)
                    .map(|cell| layout(&cell.blocks, widths[i]))
                    .unwrap_or_default()
            })
            .collect();
        let height = contents.iter().map(Vec::len).max().unwrap_or(1);
        for i in 0..height {
            let mut line = Line::styled("│", Style::DIM);
            for col in 0..columns {
                let mut content = contents[col].get(i).cloned().unwrap_or_default();
                if row.get(col).is_some_and(|c| c.heading) {
                    for span in &mut content.spans {
                        span.style = span.style.with(Style::BOLD);
                    }
                }
                let extra = widths[col].saturating_sub(content.width());
                let left = match row.get(col).map(|c| c.align).unwrap_or_default() {
                    Align::Left => 0,
                    Align::Center => extra / 2,
                    Align::Right => extra,
                };
                line.push(&" ".repeat(left + 1), Style::default());
                line.append(&content);
                line.push(&" ".repeat(extra - left + 1), Style::default());
                line.push("│", Style::DIM);
            }
            out.push(line);
        }
        if row_index + 1 < rows.len() {
            out.push(table_border(&widths, '├', '┼', '┤'));
        }
    }
    out.push(table_border(&widths, '└', '┴', '┘'));
    out
}
fn stacked_table(rows: &[Vec<Cell>], width: usize, columns: usize) -> Vec<Line> {
    let header = rows.first().filter(|r| r.iter().any(|c| c.heading));
    let labels: Vec<String> = (0..columns)
        .map(|i| {
            header
                .and_then(|r| r.get(i))
                .map(|c| {
                    layout(&c.blocks, width)
                        .iter()
                        .map(Line::text)
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("Column {}", i + 1))
        })
        .collect();
    let skip = usize::from(header.is_some() && rows.len() > 1);
    let mut out = Vec::new();
    for (row_index, row) in rows.iter().skip(skip).enumerate() {
        if row_index > 0 {
            out.push(Line::default());
        }
        out.extend(wrap_inline(
            &[inline(&format!("Row {}", row_index + 1), Style::DIM)],
            width,
        ));
        for (col, label) in labels.iter().enumerate() {
            out.extend(wrap_inline(
                &[inline(&format!("{label}:"), Style::BOLD)],
                width,
            ));
            if let Some(cell) = row.get(col) {
                out.extend(prefixed(&cell.blocks, width, "  ", "  "));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rendered(source: &str, width: usize) -> Vec<Line> {
        layout(&Document::parse(source, true).blocks, width)
    }
    fn plain(source: &str, width: usize) -> String {
        rendered(source, width)
            .iter()
            .map(Line::text)
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn fits(lines: &[Line], width: usize) {
        for line in lines {
            assert!(
                line.width() <= width,
                "{} > {width}: {:?}",
                line.width(),
                line.text()
            );
        }
    }

    #[test]
    fn commonmark_headings_and_paragraphs() {
        let source = "# Mixed Case\n\nSetext\n======\n\n## Two\n\n### Three\n\n#### Four\n\n##### Five\n\n###### Six\n\nsoft\nbreak\n\nhard  \nbreak\n\n---";
        let result = plain(source, 40);
        assert!(result.contains("Mixed Case"), "{result}");
        assert!(result.contains("soft break"));
        assert!(result.contains("hard\nbreak"));
        for name in ["Setext", "Two", "Three", "Four", "Five", "Six"] {
            assert!(result.contains(name));
        }
        assert!(rendered(source, 40)[0].spans[0].style.0 & Style::BOLD.0 != 0);
        assert!(result.contains(&"─".repeat(40)));
    }
    #[test]
    fn nested_inline_styles_and_literal_punctuation() {
        let source = r"**bold *nested*** __strong__ ~~gone~~ ==marked== ++added++ `a_b * c` a_b_c \*literal\* &amp; H~2~O x^2^ ||visible||";
        let result = plain(source, 160);
        for text in [
            "bold nested",
            "strong",
            "gone",
            "marked",
            "added",
            "a_b * c",
            "a_b_c",
            "*literal*",
            "&",
            "H₂O",
            "x²",
            "visible",
        ] {
            assert!(result.contains(text), "missing {text}: {result}");
        }
        let lines = rendered(source, 160);
        assert!(lines
            .iter()
            .flat_map(|l| &l.spans)
            .any(|s| s.text.contains("nested") && s.style == Style::BOLD.with(Style::ITALIC)));
        assert!(lines
            .iter()
            .flat_map(|l| &l.spans)
            .any(|s| s.style.0 & Style::STRIKE.0 != 0));
    }
    #[test]
    fn lists_tasks_quotes_and_definitions() {
        let source = "3. first\n4. second\n   - nested\n     - deeper\n\n- [x] done\n- [ ] pending\n\n> quote\n>\n> - inside\n\nTerm\n\n: definition\n";
        let result = plain(source, 50);
        for text in [
            "3. first",
            "4. second",
            "• nested",
            "• deeper",
            "[x] done",
            "[ ] pending",
            "│ quote",
            "inside",
            "Term",
            "definition",
        ] {
            assert!(result.contains(text), "missing {text}: {result}");
        }
    }
    #[test]
    fn code_preserves_whitespace_and_does_not_parse_markdown() {
        let source = "```rust\nfn a_b() {\n\tlet x = \"**literal**\";\n\n}\n```\n\n    indented_*_code\n\n~~~unknown\nunclosed_fence*";
        let result = plain(source, 70);
        assert!(
            result.contains("│     let x = \"**literal**\";"),
            "{result}"
        );
        assert!(result.contains("│ fn a_b() {"));
        assert!(result.contains("│ indented_*_code"));
        assert!(result.contains("unknown\n│ unclosed_fence*"));
        let wrapped = rendered("```\n0123456789abcdefghijk\n```", 10);
        fits(&wrapped, 10);
        assert!(wrapped.iter().any(|l| l.text().contains("↪")));
        let text: String = wrapped
            .iter()
            .map(|l| l.text().replace("│ ", "").replace("↪ ", ""))
            .collect();
        assert_eq!(text, "0123456789abcdefghijk");
    }
    #[test]
    fn links_are_numbered_once_and_unresolved_references_remain() {
        let source = "[first](https://example.com/a) [second][ref] ![diagram](image.png) [unresolved][no]\n\n[ref]: https://example.com/a\n";
        let result = plain(source, 120);
        assert!(
            result.contains("first[1] second[1] [Image: diagram][2]"),
            "{result}"
        );
        assert!(result.contains("[unresolved][no]"));
        assert_eq!(result.matches("https://example.com/a").count(), 1);
        assert!(result.contains("Links\n\n[1] https://example.com/a\n[2] image.png"));
    }
    #[test]
    fn footnotes_are_separate_from_links() {
        let source = "A note[^n] and a [link](https://example.com).\n\n[^n]: Footnote **body**.\n\nAnother^[Inline footnote].";
        let result = plain(source, 90);
        assert!(result.contains("A note[^1]"), "{result}");
        assert!(result.contains("Footnotes"));
        assert!(result.contains("[^1] Footnote body."));
        assert!(result.contains("[^2] Inline footnote"));
        assert!(!result.contains("#fn"));
        assert!(result.contains("[1] https://example.com"));
    }
    #[test]
    fn tables_align_and_fall_back_without_losing_cells() {
        let source = "| Name | Count |\n| :--- | ---: |\n| apples | 7 |\n| **pears** | 120 |";
        let wide = plain(source, 50);
        assert!(wide.contains("┌"));
        assert!(wide.contains("│ apples │     7 │"), "{wide}");
        let narrow = plain(source, 14);
        assert!(
            narrow.contains("Row 1\nName:\n  apples\nCount:\n  7"),
            "{narrow}"
        );
        assert!(narrow.contains("pears") && narrow.contains("120"));
        for width in 1..60 {
            fits(&rendered(source, width), width);
        }
    }
    #[test]
    fn common_extensions_and_media_fallbacks() {
        let source = "> [!WARNING]\n> Be careful\n\n[[note|Wiki title]] :smile:\n\n$x_1$\n\n$$\ny = x^2\n$$\n\n```mermaid\ngraph TD; A-->B\n```\n\n>>>\nMultiline quote\n>>>";
        let result = plain(source, 80);
        for text in [
            "Warning",
            "Be careful",
            "Wiki title[1]",
            "😄",
            "$x_1$",
            "y = x^2",
            "mermaid",
            "graph TD; A-->B",
            "Multiline quote",
        ] {
            assert!(result.contains(text), "missing {text}: {result}");
        }
    }
    #[test]
    fn front_matter_is_verbatim_and_unclosed_delimiters_are_markdown() {
        for delimiter in ["---", "+++"] {
            let result = plain(
                &format!("{delimiter}\ntitle: *literal*\n{delimiter}\n\n# Body"),
                60,
            );
            assert!(result.contains("metadata"));
            assert!(result.contains("title: *literal*"));
            assert!(result.contains("Body"));
        }
        assert!(plain("---\n# Still a heading", 40).contains("Still a heading"));
    }
    #[test]
    fn embedded_html_is_readable_and_passive() {
        let source = "<details><summary>Summary</summary><p>Hello <strong>world</strong><br>next &amp; last</p><table><tr><th>Key</th><th>Value</th></tr><tr><td>html</td><td>table</td></tr></table></details>\n\n<p><kbd>Esc</kbd> H<sub>2</sub>O <em>italic</em></p><script>alert('NEVER')</script><style>.NEVER { color: red }</style>";
        let result = plain(source, 70);
        for text in [
            "Summary",
            "Hello world\nnext & last",
            "html",
            "table",
            "Esc",
            "H₂O",
            "italic",
        ] {
            assert!(result.contains(text), "missing {text}: {result}");
        }
        assert!(!result.contains("NEVER"));
        assert!(!result.contains("<strong>"));
    }
    #[test]
    fn literal_text_files_keep_markdown_and_html_source() {
        let source = "# Title\n  **literal** a_b\n<table>source</table>\n";
        let doc = Document::parse(source, false);
        assert_eq!(
            layout(&doc.blocks, 80)
                .iter()
                .map(Line::text)
                .collect::<Vec<_>>()
                .join("\n"),
            source
        );
    }
    #[test]
    fn unicode_and_long_tokens_stay_inside_the_pane() {
        let source = "# café e\u{301} 日本語 👩‍💻\n\n**日本語** 👩‍💻👩‍💻 e\u{301}e\u{301} [long](https://example.com/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa)\n\n> - > deeply nested\n\n```\n    wide日本語 👩‍💻 e\u{301}\n```";
        for width in 4..90 {
            let lines = rendered(source, width);
            fits(&lines, width);
            for line in &lines {
                assert!(!line.text().starts_with('\u{200d}'));
                assert!(!line.text().starts_with('\u{301}'));
            }
        }
        assert!(plain(source, 80).contains("👩‍💻"));
    }
    #[test]
    fn document_controls_cannot_become_terminal_commands() {
        let source = "# Header\n\n\x1b]52;c;secret\x07\n\n```lang\x1b[31m\n\x1b[2J\n```";
        let output: String = rendered(source, 80).iter().map(|l| l.ansi(80)).collect();
        assert!(!output.contains("\x1b]52"));
        assert!(!output.contains("\x1b[2J"));
        assert!(!output.contains("\x1b[31m"));
        assert!(!output.contains("38;"));
        assert!(!output.contains("48;"));
        assert!(output.ends_with(' ') || output.ends_with("\x1b[0m"));
    }
    #[test]
    fn preserved_spacing_math_and_showcase_widths() {
        assert!(plain("`a  b`", 80).contains("a  b"));
        let math = plain("$$\nx = 1\ny = 2\n$$", 80);
        assert!(math.contains("│ x = 1\n│ y = 2"), "{math}");
        assert!(plain("<custom><p>first</p><p>second</p></custom>", 80).contains("first\n\nsecond"));
        let source = include_str!("../tests/fixtures/markdown-showcase.md");
        for width in [1, 4, 12, 26, 56, 86, 120] {
            fits(&rendered(source, width), width);
        }
    }
    #[test]
    fn cache_reflows_and_invalidates_by_revision_and_format() {
        let mut cache = PreviewCache::default();
        let source = vec!["**bold** and a long paragraph that wraps".to_owned()];
        assert!(cache.lines(&source, 0, true, 80)[0]
            .text()
            .starts_with("bold"));
        assert!(cache.lines(&source, 0, true, 10).len() > 1);
        fits(cache.lines(&source, 0, true, 10), 10);
        assert!(cache.lines(&source, 0, false, 80)[0]
            .text()
            .starts_with("**bold**"));
        assert_eq!(
            cache.lines(&["changed".into()], 1, true, 80)[0].text(),
            "changed"
        );
    }
}
