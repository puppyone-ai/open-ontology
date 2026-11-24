//! Markdown parser (`oo-md`): CommonMark + tables, strikethrough, task lists
//! and footnotes through pulldown-cmark, plus a hand-detected frontmatter
//! block (10 §4.4, §3.5). Heading chains come from ATX and Setext headings.
//!
//! Block mapping:
//! - heading (level 1..6; `hpath` ends with the heading itself);
//! - paragraph; list item (one block per item, nested items are separate
//!   blocks whose `parent` is the enclosing item); blockquote (one `quote`
//!   block with the flattened text of everything inside); fenced or indented
//!   code (`code`, fences excluded from the text); table (`table` block with a
//!   `TableGrid`); footnote definition (`footnote`);
//! - frontmatter (`frontmatter`, text = the YAML between the fences).
//!
//! Raw HTML is skipped (one `DOCS_PARSE_HTML_SKIPPED` warning per file).

use std::ops::Range;

use oo_core::docir::{
    Anchor, BlockKind, DocFormat, DocMeta, MAX_HPATH, ParserId, TableCell, TableGrid,
};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use serde_json::Value;

use super::decode::decode_text;
use super::frontmatter::{parse_simple_yaml, split};
use super::lines::LineIndex;
use super::{ParseError, blob_b3, clip_chars, parser_info};
use crate::normalize::{IrFragment, IrWarning, RawBlock};

const PROFILE: &str = "oo-md/1;cmark=0.13.6;ext=tables,strikethrough,tasklists,footnotes;frontmatter=simple-yaml/1;offsets=scalar-half-open;html=skipped;encodings=utf-8,utf-8-bom,utf-16-bom";

const HPATH_ITEM_MAX: usize = 512;

enum Frame {
    Heading {
        level: u8,
        range: Range<usize>,
        text: String,
    },
    /// A paragraph that becomes its own block.
    Para {
        range: Range<usize>,
        text: String,
    },
    /// A paragraph whose text flows into the enclosing item, quote or
    /// footnote.
    Flow,
    Item {
        idx: usize,
        start: usize,
        own_end: usize,
        text: String,
    },
    Quote {
        range: Range<usize>,
        text: String,
    },
    Code {
        range: Range<usize>,
        text: String,
        item: Option<usize>,
    },
    Table(TableState),
    Footnote {
        range: Range<usize>,
        text: String,
    },
    List,
}

#[derive(Default)]
struct TableState {
    start: usize,
    end: usize,
    rows: Vec<Vec<String>>,
    header_rows: u32,
    row: Vec<String>,
    cell: String,
}

struct Builder<'a> {
    index: &'a LineIndex<'a>,
    blocks: Vec<RawBlock>,
    stack: Vec<Frame>,
    headings: Vec<(u8, String)>,
    html_skipped: bool,
    first_h1: Option<String>,
}

impl Builder<'_> {
    fn hpath(&self) -> Vec<String> {
        let mut path: Vec<String> = self
            .headings
            .iter()
            .map(|(_, t)| clip_chars(t, HPATH_ITEM_MAX))
            .collect();
        path.truncate(MAX_HPATH);
        path
    }

    fn push(
        &mut self,
        kind: BlockKind,
        text: String,
        anchor: Anchor,
        parent: Option<usize>,
    ) -> usize {
        let block = RawBlock {
            kind,
            level: None,
            text,
            hpath: self.hpath(),
            parent,
            anchor,
            table: None,
            flags: Vec::new(),
            exact: false,
        };
        self.blocks.push(block);
        self.blocks.len() - 1
    }

    fn enclosing_item(&self) -> Option<usize> {
        self.stack.iter().rev().find_map(|f| match f {
            Frame::Item { idx, .. } => Some(*idx),
            _ => None,
        })
    }

    fn in_quote(&self) -> bool {
        self.stack.iter().any(|f| matches!(f, Frame::Quote { .. }))
    }

    /// Where inline text goes: the outermost quote swallows everything
    /// inside it, otherwise the innermost frame that owns a text buffer.
    fn sink(&mut self) -> Option<(&mut String, Option<&mut usize>)> {
        let quote = self
            .stack
            .iter()
            .position(|f| matches!(f, Frame::Quote { .. }));
        let pos = match quote {
            Some(p) => p,
            None => self
                .stack
                .iter()
                .rposition(|f| !matches!(f, Frame::Flow | Frame::List))?,
        };
        match &mut self.stack[pos] {
            Frame::Heading { text, .. }
            | Frame::Para { text, .. }
            | Frame::Quote { text, .. }
            | Frame::Code { text, .. }
            | Frame::Footnote { text, .. } => Some((text, None)),
            Frame::Item { text, own_end, .. } => Some((text, Some(own_end))),
            Frame::Table(t) => Some((&mut t.cell, None)),
            Frame::Flow | Frame::List => None,
        }
    }

    fn text(&mut self, s: &str, end: usize) {
        if let Some((buf, own_end)) = self.sink() {
            buf.push_str(s);
            if let Some(oe) = own_end {
                *oe = (*oe).max(end);
            }
        }
    }

    /// Separate flowing blocks inside quotes and items with a newline.
    fn newline_if_needed(&mut self) {
        if let Some((buf, _)) = self.sink()
            && !buf.is_empty()
            && !buf.ends_with('\n')
        {
            buf.push('\n');
        }
    }

    fn start(&mut self, tag: Tag<'_>, range: Range<usize>) {
        match tag {
            Tag::Heading { level, .. } => self.stack.push(Frame::Heading {
                level: level as u8,
                range,
                text: String::new(),
            }),
            Tag::Paragraph => {
                let flows = self.stack.iter().any(|f| {
                    matches!(
                        f,
                        Frame::Item { .. } | Frame::Quote { .. } | Frame::Footnote { .. }
                    )
                });
                if flows {
                    self.newline_if_needed();
                    self.stack.push(Frame::Flow);
                } else {
                    self.stack.push(Frame::Para {
                        range,
                        text: String::new(),
                    });
                }
            }
            Tag::BlockQuote(_) => {
                if !self.in_quote() {
                    self.stack.push(Frame::Quote {
                        range,
                        text: String::new(),
                    });
                } else {
                    self.stack.push(Frame::Flow);
                }
            }
            Tag::CodeBlock(_) => {
                let item = if self.in_quote() {
                    None
                } else {
                    self.enclosing_item()
                };
                self.stack.push(Frame::Code {
                    range,
                    text: String::new(),
                    item,
                });
            }
            Tag::List(_) => self.stack.push(Frame::List),
            Tag::Item => {
                if self.in_quote() {
                    self.newline_if_needed();
                    self.stack.push(Frame::Flow);
                } else {
                    let parent = self.enclosing_item();
                    let anchor = self.index.anchor(range.start, range.end);
                    let idx = self.push(BlockKind::ListItem, String::new(), anchor, parent);
                    self.stack.push(Frame::Item {
                        idx,
                        start: range.start,
                        own_end: range.start,
                        text: String::new(),
                    });
                }
            }
            Tag::FootnoteDefinition(_) => self.stack.push(Frame::Footnote {
                range,
                text: String::new(),
            }),
            Tag::Table(_) => self.stack.push(Frame::Table(TableState {
                start: range.start,
                end: range.end,
                ..TableState::default()
            })),
            Tag::TableHead | Tag::TableRow => {
                if let Some(Frame::Table(t)) = self.stack.last_mut() {
                    t.row.clear();
                }
                self.stack.push(Frame::Flow);
            }
            Tag::TableCell => {
                if let Some(Frame::Table(t)) = self
                    .stack
                    .iter_mut()
                    .rev()
                    .find(|f| matches!(f, Frame::Table(_)))
                {
                    t.cell.clear();
                }
                self.stack.push(Frame::Flow);
            }
            // Inline containers: their text flows into the current sink.
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) => {
                if let Some(Frame::Heading { level, range, text }) = self.stack.pop() {
                    if self.in_quote() {
                        self.newline_if_needed();
                        return;
                    }
                    let text = text.trim().to_owned();
                    while self.headings.last().is_some_and(|(l, _)| *l >= level) {
                        self.headings.pop();
                    }
                    self.headings.push((level, text.clone()));
                    if level == 1 && self.first_h1.is_none() && !text.is_empty() {
                        self.first_h1 = Some(text.clone());
                    }
                    let anchor = self.index.anchor(range.start, range.end);
                    let idx = self.push(BlockKind::Heading, text, anchor, None);
                    self.blocks[idx].level = Some(level);
                }
            }
            TagEnd::Paragraph => match self.stack.pop() {
                Some(Frame::Para { range, text }) => {
                    let text = text.trim().to_owned();
                    let anchor = self.index.anchor(range.start, range.end);
                    self.push(BlockKind::Paragraph, text, anchor, None);
                }
                Some(Frame::Flow) => self.newline_if_needed(),
                other => self.stack.extend(other),
            },
            TagEnd::BlockQuote(_) => match self.stack.pop() {
                Some(Frame::Quote { range, text }) => {
                    let anchor = self.index.anchor(range.start, range.end);
                    self.push(BlockKind::Quote, text.trim().to_owned(), anchor, None);
                }
                other => self.stack.extend(other),
            },
            TagEnd::CodeBlock => {
                if let Some(Frame::Code { range, text, item }) = self.stack.pop() {
                    let text = text.strip_suffix('\n').unwrap_or(&text).to_owned();
                    if self.in_quote() {
                        self.newline_if_needed();
                    } else {
                        let anchor = self.index.anchor(range.start, range.end);
                        self.push(BlockKind::Code, text, anchor, item);
                    }
                }
            }
            TagEnd::List(_) => {
                self.stack.pop();
            }
            TagEnd::Item => match self.stack.pop() {
                Some(Frame::Item {
                    idx,
                    start,
                    own_end,
                    text,
                }) => {
                    let end = if own_end > start { own_end } else { start };
                    self.blocks[idx].anchor = self.index.anchor(start, end);
                    self.blocks[idx].text = text.trim().to_owned();
                }
                Some(Frame::Flow) => self.newline_if_needed(),
                other => self.stack.extend(other),
            },
            TagEnd::FootnoteDefinition => {
                if let Some(Frame::Footnote { range, text }) = self.stack.pop() {
                    let anchor = self.index.anchor(range.start, range.end);
                    self.push(BlockKind::Footnote, text.trim().to_owned(), anchor, None);
                }
            }
            TagEnd::TableCell => {
                self.stack.pop();
                if let Some(Frame::Table(t)) = self
                    .stack
                    .iter_mut()
                    .rev()
                    .find(|f| matches!(f, Frame::Table(_)))
                {
                    let cell = t.cell.trim().to_owned();
                    t.row.push(cell);
                    t.cell.clear();
                }
            }
            TagEnd::TableHead => {
                self.stack.pop();
                if let Some(Frame::Table(t)) = self.stack.last_mut() {
                    t.rows.push(std::mem::take(&mut t.row));
                    t.header_rows = 1;
                }
            }
            TagEnd::TableRow => {
                self.stack.pop();
                if let Some(Frame::Table(t)) = self.stack.last_mut() {
                    t.rows.push(std::mem::take(&mut t.row));
                }
            }
            TagEnd::Table => {
                if let Some(Frame::Table(t)) = self.stack.pop() {
                    self.finish_table(t);
                }
            }
            _ => {}
        }
    }

    fn finish_table(&mut self, t: TableState) {
        let cols = t.rows.iter().map(Vec::len).max().unwrap_or(0);
        let mut cells = Vec::new();
        for (r, row) in t.rows.iter().enumerate() {
            for (c, text) in row.iter().enumerate() {
                cells.push(TableCell {
                    r: r as u32,
                    c: c as u32,
                    rs: None,
                    cs: None,
                    text: text.clone(),
                    header: ((r as u32) < t.header_rows).then_some(true),
                });
            }
        }
        let text = t
            .rows
            .iter()
            .map(|row| row.join(" | "))
            .collect::<Vec<_>>()
            .join("\n");
        if self.in_quote() {
            self.text(&format!("{text}\n"), t.end);
            return;
        }
        let anchor = self.index.anchor(t.start, t.end);
        let parent = self.enclosing_item();
        let idx = self.push(BlockKind::Table, text, anchor, parent);
        self.blocks[idx].table = Some(TableGrid {
            rows: t.rows.len() as u32,
            cols: cols as u32,
            header_rows: t.header_rows,
            cells,
        });
    }
}

pub fn parse_markdown_fragment(bytes: &[u8]) -> Result<IrFragment, ParseError> {
    let decoded = decode_text(bytes)?;
    let text = decoded.text.as_str();
    let index = LineIndex::new(text);
    let mut warnings: Vec<IrWarning> = Vec::new();
    let mut meta = DocMeta {
        encoding: Some(decoded.encoding),
        ..DocMeta::default()
    };

    let mut builder = Builder {
        index: &index,
        blocks: Vec::new(),
        stack: Vec::new(),
        headings: Vec::new(),
        html_skipped: false,
        first_h1: None,
    };

    let mut body_start = 0usize;
    if let Some(fm) = split(text) {
        body_start = fm.body_start;
        let yaml = &text[fm.inner.0..fm.inner.1];
        let parsed = parse_simple_yaml(yaml);
        let anchor = index.anchor(0, fm.block_end);
        let idx = builder.push(
            BlockKind::Frontmatter,
            yaml.trim_end().to_owned(),
            anchor,
            None,
        );
        if parsed.unsupported > 0 {
            warnings.push(IrWarning {
                code: "DOCS_PARSE_FRONTMATTER_UNSUPPORTED".to_owned(),
                block: Some(idx),
            });
        }
        if !parsed.map.is_empty() {
            apply_meta(&mut meta, &parsed.map);
            meta.frontmatter = Some(parsed.map);
        }
    }

    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    let body = &text[body_start..];
    for (event, range) in Parser::new_ext(body, options).into_offset_iter() {
        let range = range.start + body_start..range.end + body_start;
        match event {
            Event::Start(tag) => builder.start(tag, range),
            Event::End(tag) => builder.end(tag),
            Event::Text(t) => builder.text(&t, range.end),
            Event::Code(t) => builder.text(&t, range.end),
            Event::InlineMath(t) | Event::DisplayMath(t) => builder.text(&t, range.end),
            Event::SoftBreak | Event::HardBreak => builder.text("\n", range.end),
            Event::Html(_) | Event::InlineHtml(_) => builder.html_skipped = true,
            Event::FootnoteReference(_) | Event::Rule | Event::TaskListMarker(_) => {}
        }
    }

    if builder.html_skipped {
        warnings.push(IrWarning {
            code: "DOCS_PARSE_HTML_SKIPPED".to_owned(),
            block: None,
        });
    }
    if meta.title.is_none() {
        meta.title = builder.first_h1.as_deref().map(|t| clip_chars(t, 512));
    }
    let blocks = builder.blocks;
    if blocks.is_empty() {
        warnings.push(IrWarning {
            code: "DOCS_PARSE_EMPTY".to_owned(),
            block: None,
        });
    }
    Ok(IrFragment {
        blob_b3: blob_b3(bytes),
        format: DocFormat::Md,
        parser: parser_info(ParserId::OoMd, PROFILE),
        meta,
        blocks,
        warnings,
    })
}

/// `YYYY-MM-DD` optionally followed by `THH:MM:SS[.f+]Z` (the schema's
/// `date_or_dt`).
pub fn is_date_or_dt(s: &str) -> bool {
    let b = s.as_bytes();
    let digit = |i: usize| b.get(i).is_some_and(u8::is_ascii_digit);
    let date = b.len() >= 10
        && (0..4).all(digit)
        && b[4] == b'-'
        && (5..7).all(digit)
        && b[7] == b'-'
        && (8..10).all(digit);
    if !date {
        return false;
    }
    if b.len() == 10 {
        return true;
    }
    let rest = &s[10..];
    let Some(t) = rest.strip_prefix('T').and_then(|r| r.strip_suffix('Z')) else {
        return false;
    };
    let (hms, frac) = match t.split_once('.') {
        Some((h, f)) => (h, Some(f)),
        None => (t, None),
    };
    let hb = hms.as_bytes();
    hb.len() == 8
        && hb[2] == b':'
        && hb[5] == b':'
        && hb
            .iter()
            .enumerate()
            .all(|(i, c)| i == 2 || i == 5 || c.is_ascii_digit())
        && frac.is_none_or(|f| !f.is_empty() && f.bytes().all(|c| c.is_ascii_digit()))
}

fn string_list(v: &Value) -> Vec<String> {
    match v {
        Value::String(s) => vec![clip_chars(s, 256)],
        Value::Array(items) => items
            .iter()
            .filter_map(|i| i.as_str().map(|s| clip_chars(s, 256)))
            .take(64)
            .collect(),
        _ => Vec::new(),
    }
}

fn apply_meta(meta: &mut DocMeta, map: &std::collections::BTreeMap<String, Value>) {
    if let Some(Value::String(t)) = map.get("title")
        && !t.trim().is_empty()
    {
        meta.title = Some(clip_chars(t.trim(), 512));
    }
    for key in ["authors", "author"] {
        if let Some(v) = map.get(key) {
            let list = string_list(v);
            if !list.is_empty() {
                meta.authors = list;
                break;
            }
        }
    }
    let date = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| map.get(*k).and_then(Value::as_str))
            .filter(|s| is_date_or_dt(s))
            .map(str::to_owned)
    };
    meta.created = date(&["created", "date"]);
    meta.modified = date(&["modified", "updated"]);
}

/// Parse a `.md` file to canonical IR.
pub fn parse_markdown(bytes: &[u8]) -> Result<crate::normalize::Normalized, ParseError> {
    Ok(crate::normalize::finish(parse_markdown_fragment(bytes)?)?)
}
