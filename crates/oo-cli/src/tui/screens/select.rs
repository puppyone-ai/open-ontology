//! Screen 2: choose tables. Catalog metadata only; the scorer is
//! `tui::scorer`. The saved result is always an explicit list.
use std::collections::BTreeSet;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::{Action, input, put};
use crate::tui::adjudication::magnitude;
use crate::tui::fx;
use crate::tui::scorer::{Scored, TableMeta, Verdict, recommended, score};
use crate::tui::theme::{self, Hint, Theme, Tone, fit, heading, width};

#[derive(Clone, Debug)]
pub struct Select {
    pub rows: Vec<Scored>,
    pub chosen: BTreeSet<String>,
    pub cursor: usize,
    pub scroll: usize,
    pub search: String,
    pub searching: bool,
    pub error: Option<String>,
    /// Animation frame: verdicts sweep in two rows a frame, the meter fills.
    pub frame: usize,
    /// Catalog metadata behind each row, for the detail panel.
    pub meta: Vec<TableMeta>,
}

impl Select {
    /// Recommendations start selected: `a` restores them after edits.
    pub fn new(tables: &[TableMeta]) -> Self {
        let rows = score(tables);
        let chosen = recommended(&rows).into_iter().collect();
        Select {
            rows,
            chosen,
            cursor: 0,
            scroll: 0,
            search: String::new(),
            searching: false,
            error: None,
            frame: 0,
            meta: tables.to_vec(),
        }
    }

    pub fn visible(&self) -> Vec<usize> {
        let needle = self.search.to_lowercase();
        (0..self.rows.len())
            .filter(|&i| needle.is_empty() || self.rows[i].table.to_lowercase().contains(&needle))
            .collect()
    }

    pub fn selection(&self) -> Vec<String> {
        self.chosen.iter().cloned().collect()
    }
}

pub fn key(state: &mut Select, key: KeyEvent) -> Action {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Action::Quit;
    }
    let visible = state.visible();
    if state.searching {
        match key.code {
            KeyCode::Esc => {
                state.search.clear();
                state.searching = false;
            }
            KeyCode::Enter => state.searching = false,
            KeyCode::Backspace => {
                state.search.pop();
            }
            KeyCode::Char(c) if !c.is_control() && state.search.len() < 128 => state.search.push(c),
            _ => (),
        }
        state.cursor = 0;
        state.scroll = 0;
        return Action::None;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => return Action::Quit,
        KeyCode::Up | KeyCode::Char('k') => state.cursor = state.cursor.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => {
            state.cursor = (state.cursor + 1).min(visible.len().saturating_sub(1));
        }
        KeyCode::PageDown => {
            state.cursor = (state.cursor + 10).min(visible.len().saturating_sub(1))
        }
        KeyCode::PageUp => state.cursor = state.cursor.saturating_sub(10),
        KeyCode::Char(' ') => {
            if let Some(&i) = visible.get(state.cursor) {
                if state.rows[i].verdict == Verdict::Skip {
                    return Action::None;
                }
                let table = state.rows[i].table.clone();
                if !state.chosen.remove(&table) {
                    state.chosen.insert(table);
                }
            }
        }
        KeyCode::Char('a') => state.chosen = recommended(&state.rows).into_iter().collect(),
        KeyCode::Char('n') => state.chosen.clear(),
        KeyCode::Char('/') => {
            state.searching = true;
            state.search.clear();
        }
        KeyCode::Enter => {
            if state.chosen.is_empty() {
                state.error = Some(NONE_SELECTED.into());
            } else {
                return Action::Next;
            }
        }
        _ => (),
    }
    Action::None
}

pub const HINTS: [Hint; 6] = [
    ("␣", "toggle", "勾选"),
    ("a", "recommended", "用推荐"),
    ("/", "search", "搜索"),
    ("n", "none", "全不选"),
    ("⏎", "save", "保存"),
    ("q", "quit", "退出"),
];

/// Frames the opening sweep takes.
pub const SWEEP_FRAMES: usize = 9;

pub fn render(state: &Select, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let inner = theme::frame(buf, area, theme, 1, &HINTS);
    if inner.height < 8 {
        return;
    }
    let frame = theme.frame(state.frame);
    let count = |v: Verdict| state.rows.iter().filter(|r| r.verdict == v).count();
    heading(
        buf,
        inner.x,
        inner.y,
        inner.width,
        theme,
        theme.t("tables", "选表"),
        &if theme.zh {
            format!("已选 {} / {}", state.chosen.len(), state.rows.len())
        } else {
            format!("{} of {} selected", state.chosen.len(), state.rows.len())
        },
    );
    // Summary line + selection meter (progress-bar style, fills on entry).
    let mut spans = vec![
        theme.span("✓ ", Tone::Ok),
        theme.span(
            format!(
                "{} {}   ",
                count(Verdict::Recommend),
                theme.t("recommend", "推荐")
            ),
            Tone::Dim,
        ),
        theme.span("? ", Tone::Warn),
        theme.span(
            format!("{} {}   ", count(Verdict::Maybe), theme.t("maybe", "待定")),
            Tone::Dim,
        ),
        theme.span("✗ ", Tone::Faint),
        theme.span(
            format!("{} {}   ", count(Verdict::Skip), theme.t("skip", "跳过")),
            Tone::Dim,
        ),
    ];
    let used: usize = spans.iter().map(ratatui::text::Span::width).sum();
    let meter_w = usize::from(inner.width).saturating_sub(used + 2).min(30);
    if meter_w >= 6 && !state.rows.is_empty() {
        let fill = state.chosen.len() as f64 / state.rows.len() as f64
            * f64::from(fx::ease_out(fx::progress(frame, SWEEP_FRAMES)));
        let (done, rest) = fx::bar(fill, meter_w - 2);
        spans.push(theme.span("▕", Tone::Faint));
        spans.push(theme.span(done, Tone::Accent));
        spans.push(theme.span(rest, Tone::Faint));
        spans.push(theme.span("▏", Tone::Faint));
    }
    put(buf, inner.x, inner.y + 1, inner.width, spans);
    let top = inner.y + 3;
    // Wide terminals get a detail panel for the row under the cursor.
    let panel = inner.width >= PANEL_MIN;
    let list_w = if panel {
        inner.width - PANEL_W - 2
    } else {
        inner.width
    };
    let list_h = usize::from(inner.bottom().saturating_sub(top + 2));
    let visible = state.visible();
    let cursor = state.cursor.min(visible.len().saturating_sub(1));
    let scroll = if cursor < state.scroll {
        cursor
    } else if cursor >= state.scroll + list_h {
        cursor + 1 - list_h
    } else {
        state.scroll
    };
    let name_w = state
        .rows
        .iter()
        .map(|r| width(&r.table))
        .max()
        .unwrap_or(10)
        .clamp(10, if list_w >= 100 { 30 } else { 22 });
    let mut y = top;
    for (pos, &i) in visible.iter().enumerate().skip(scroll).take(list_h) {
        let row = &state.rows[i];
        let on = pos == cursor;
        let picked = state.chosen.contains(&row.table);
        // Verdicts sweep in top to bottom, two rows a frame.
        let swept = frame == usize::MAX || (pos - scroll) < frame.saturating_mul(2);
        let (glyph, tone) = match (swept, row.verdict) {
            (false, _) => ("·", Tone::Faint),
            (true, Verdict::Recommend) => ("✓", Tone::Ok),
            (true, Verdict::Maybe) => ("?", Tone::Warn),
            (true, Verdict::Skip) => ("✗", Tone::Faint),
        };
        let name_tone = if row.verdict == Verdict::Skip && !picked {
            Tone::Dim
        } else {
            Tone::Ink
        };
        let rows = row.rows.map_or("—".to_owned(), magnitude);
        let reason_w = usize::from(list_w).saturating_sub(name_w + 20);
        let reason = if swept {
            fit(&row.reason.text(theme.zh), reason_w)
        } else {
            String::new()
        };
        let mut spans = vec![
            theme.span(if on { "▸ " } else { "  " }, Tone::Accent),
            theme.span(
                if picked { "◉ " } else { "○ " },
                if picked { Tone::Accent } else { Tone::Faint },
            ),
            theme.span(format!("{glyph} "), tone),
            theme.span(theme::pad(&row.table, name_w), name_tone),
            theme.span(format!("{rows:>6} "), Tone::Dim),
            theme.span(
                format!("{} ", fx::magnitude_bar(row.rows)),
                if picked { Tone::Ember } else { Tone::Faint },
            ),
            theme.span(reason, Tone::Dim),
        ];
        if on {
            spans[3] =
                ratatui::text::Span::styled(theme::pad(&row.table, name_w), theme.selected());
        }
        put(buf, inner.x, y, list_w, spans);
        y += 1;
    }
    if visible.len() > list_h {
        let more = visible.len() - (scroll + list_h).min(visible.len());
        put(
            buf,
            inner.x + 4,
            y,
            inner.width,
            vec![theme.span(
                if theme.zh {
                    format!("↓ 下面还有 {more} 张")
                } else {
                    format!("↓ {more} more below")
                },
                Tone::Faint,
            )],
        );
    }
    if panel && let Some(&i) = visible.get(cursor) {
        let px = inner.x + list_w + 1;
        for py in top..inner.bottom().saturating_sub(1) {
            buf[(px, py)]
                .set_symbol("┊")
                .set_style(theme.style(Tone::Faint));
        }
        detail(
            state,
            i,
            Rect::new(
                px + 2,
                top,
                PANEL_W - 1,
                inner.bottom().saturating_sub(top + 1),
            ),
            buf,
            theme,
        );
    }
    let foot = inner.bottom() - 1;
    if state.searching || !state.search.is_empty() {
        let label = theme.t("search", "搜索");
        let lw = width(label) as u16 + 1;
        put(buf, inner.x, foot, lw, vec![theme.span(label, Tone::Dim)]);
        input(
            buf,
            inner.x + lw,
            foot,
            inner.width.saturating_sub(lw),
            theme,
            &state.search,
            state.searching,
        );
    } else if let Some(error) = &state.error {
        let text = if error == NONE_SELECTED {
            theme.t(NONE_SELECTED, "至少选一张表").to_owned()
        } else {
            error.clone()
        };
        put(
            buf,
            inner.x,
            foot,
            inner.width,
            vec![theme.span(format!("✗ {text}"), Tone::Bad)],
        );
    } else {
        put(
            buf,
            inner.x,
            foot,
            inner.width,
            vec![theme.span(
                theme.t(
                    "catalog metadata only · no rows read · nothing leaves this machine",
                    "只用目录元数据 · 不读数据行 · 不出本机",
                ),
                Tone::Faint,
            )],
        );
    }
}

/// Width at which the detail panel appears, and its width.
const PANEL_MIN: u16 = 110;
const PANEL_W: u16 = 38;

/// Thousands separators for an exact count.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// What we know about one table: size, keys, who points at it and what it
/// points at, and why it is (not) recommended.
fn detail(state: &Select, row: usize, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let scored = &state.rows[row];
    let Some(meta) = state.meta.iter().find(|m| m.qualified() == scored.table) else {
        return;
    };
    let w = usize::from(area.width);
    let mut y = area.y;
    let mut line = |spans: Vec<ratatui::text::Span<'static>>, y: &mut u16| {
        if *y < area.bottom() {
            put(buf, area.x, *y, area.width, spans);
            *y += 1;
        }
    };
    line(
        vec![theme.span(fit(&scored.table, w), Tone::Accent)],
        &mut y,
    );
    let picked = state.chosen.contains(&scored.table);
    line(
        vec![theme.span(
            if picked {
                theme.t("◉ selected", "◉ 已选")
            } else {
                theme.t("○ not selected", "○ 未选")
            },
            if picked { Tone::Accent } else { Tone::Faint },
        )],
        &mut y,
    );
    y += 1;
    let label = |t: &'static str| theme.span(theme::pad(t, 10), Tone::Dim);
    let rows = meta.rows.map_or_else(
        || theme.t("unknown", "未知").to_owned(),
        |n| format!("{} ({})", grouped(n), magnitude(n)),
    );
    line(
        vec![label(theme.t("rows", "行数")), theme.span(rows, Tone::Ink)],
        &mut y,
    );
    let keys = match (meta.has_primary_key, meta.has_unique_key) {
        (true, _) => theme.t("primary key", "有主键"),
        (false, true) => theme.t("unique key only", "只有唯一键"),
        (false, false) => theme.t("none declared", "未声明"),
    };
    line(
        vec![
            label(theme.t("keys", "键")),
            theme.span(
                keys,
                if meta.has_primary_key || meta.has_unique_key {
                    Tone::Ink
                } else {
                    Tone::Warn
                },
            ),
        ],
        &mut y,
    );
    if meta.partition_child {
        line(
            vec![
                label(theme.t("partition", "分区")),
                theme.span(
                    theme.t("child of a partitioned table", "分区子表"),
                    Tone::Warn,
                ),
            ],
            &mut y,
        );
    }
    y += 1;
    let referenced: Vec<String> = state
        .meta
        .iter()
        .filter(|m| m.references.contains(&scored.table))
        .map(TableMeta::qualified)
        .collect();
    for (title, list) in [
        (theme.t("used by", "被引用"), &referenced),
        (theme.t("points to", "引用"), &meta.references),
    ] {
        line(
            vec![
                theme.span(format!("{title} "), Tone::Dim),
                theme.span(list.len().to_string(), Tone::Ink),
            ],
            &mut y,
        );
        for name in list.iter().take(4) {
            let on = state.chosen.contains(name);
            line(
                vec![
                    theme.span(
                        if on { "  ◉ " } else { "  ○ " },
                        if on { Tone::Accent } else { Tone::Faint },
                    ),
                    theme.span(fit(name, w.saturating_sub(4)), Tone::Ink),
                ],
                &mut y,
            );
        }
        if list.len() > 4 {
            line(
                vec![theme.span(
                    if theme.zh {
                        format!("    … 还有 {} 张", list.len() - 4)
                    } else {
                        format!("    … {} more", list.len() - 4)
                    },
                    Tone::Faint,
                )],
                &mut y,
            );
        }
    }
    // A recommended table whose neighbours are left out loses its links.
    let missing = referenced
        .iter()
        .chain(&meta.references)
        .filter(|n| !state.chosen.contains(*n))
        .count();
    y += 1;
    let (glyph, tone) = match scored.verdict {
        Verdict::Recommend => ("✓", Tone::Ok),
        Verdict::Maybe => ("?", Tone::Warn),
        Verdict::Skip => ("✗", Tone::Faint),
    };
    line(
        vec![
            theme.span(format!("{glyph} "), tone),
            theme.span(
                fit(&scored.reason.text(theme.zh), w.saturating_sub(2)),
                Tone::Ink,
            ),
        ],
        &mut y,
    );
    if picked && missing > 0 {
        line(
            vec![theme.span(
                fit(
                    &if theme.zh {
                        format!("! {missing} 张相邻表未选，关系会断开")
                    } else {
                        format!("! {missing} linked table(s) not selected; their joins drop")
                    },
                    w,
                ),
                Tone::Warn,
            )],
            &mut y,
        );
    }
}

const NONE_SELECTED: &str = "select at least one table";

pub fn demo_tables() -> Vec<TableMeta> {
    let value: serde_json::Value =
        serde_json::from_str(include_str!("../demo-catalog.json")).unwrap_or_default();
    crate::tui::scorer::from_catalog(&value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::super::snapshot;
    use super::*;
    use crossterm::event::KeyEvent;

    #[test]
    fn keys_toggle_accept_and_search() {
        let mut s = Select::new(&demo_tables());
        let recommended = s.chosen.len();
        assert!(recommended > 5);
        key(&mut s, KeyEvent::from(KeyCode::Char(' ')));
        assert_eq!(s.chosen.len(), recommended - 1);
        key(&mut s, KeyEvent::from(KeyCode::Char('a')));
        assert_eq!(s.chosen.len(), recommended);
        key(&mut s, KeyEvent::from(KeyCode::Char('/')));
        for c in "log".chars() {
            key(&mut s, KeyEvent::from(KeyCode::Char(c)));
        }
        key(&mut s, KeyEvent::from(KeyCode::Enter));
        assert_eq!(s.visible().len(), 1);
        key(&mut s, KeyEvent::from(KeyCode::Char(' ')));
        assert!(
            !s.chosen.contains("lg.shipment_log"),
            "rule-skipped rows cannot be selected"
        );
        key(&mut s, KeyEvent::from(KeyCode::Char('n')));
        assert_eq!(key(&mut s, KeyEvent::from(KeyCode::Enter)), Action::None);
        assert!(s.error.is_some());
    }

    #[test]
    fn snapshots() {
        let mut state = Select::new(&demo_tables());
        state.cursor = 2;
        snapshot::all_sizes("2-tables", |area, buf, theme| {
            render(&state, area, buf, theme)
        });
        let mut zh = Select::new(&demo_tables());
        zh.searching = true;
        zh.search = "ship".into();
        snapshot::check(
            "2-tables-search",
            80,
            24,
            Theme::MONO.zh(),
            |area, buf, theme| render(&zh, area, buf, theme),
        );
    }

    #[test]
    fn opening_sweep_frames() {
        let state = Select::new(&demo_tables());
        let frames: Vec<(usize, &str)> = (0..SWEEP_FRAMES)
            .step_by(2)
            .map(|f| (f, "verdicts sweep in, meter fills"))
            .chain([(usize::MAX, "still")])
            .collect();
        snapshot::frames("2-tables", 80, 24, false, &frames, |f, area, buf, theme| {
            let mut s = state.clone();
            s.frame = f;
            render(&s, area, buf, theme);
        });
    }
}
