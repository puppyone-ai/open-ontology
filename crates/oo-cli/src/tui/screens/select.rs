//! Screen 2: choose tables. Catalog metadata only; the scorer is
//! `tui::scorer`. The saved result is always an explicit list.
use std::collections::BTreeSet;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::{Action, input, put};
use crate::tui::adjudication::magnitude;
use crate::tui::scorer::{Scored, TableMeta, Verdict, recommended, score};
use crate::tui::theme::{self, Theme, Tone, fit, heading, width};

#[derive(Clone, Debug)]
pub struct Select {
    pub rows: Vec<Scored>,
    pub chosen: BTreeSet<String>,
    pub cursor: usize,
    pub scroll: usize,
    pub search: String,
    pub searching: bool,
    pub zh: bool,
    pub error: Option<String>,
}

impl Select {
    /// Recommendations start selected: `a` restores them after edits.
    pub fn new(tables: &[TableMeta], zh: bool) -> Self {
        let rows = score(tables);
        let chosen = recommended(&rows).into_iter().collect();
        Select {
            rows,
            chosen,
            cursor: 0,
            scroll: 0,
            search: String::new(),
            searching: false,
            zh,
            error: None,
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
                state.error = Some("select at least one table".into());
            } else {
                return Action::Next;
            }
        }
        _ => (),
    }
    Action::None
}

pub const HINTS: [(&str, &str); 6] = [
    ("␣", "toggle"),
    ("a", "accept recommendations"),
    ("/", "search"),
    ("n", "none"),
    ("⏎", "save list"),
    ("q", "quit"),
];

pub fn render(state: &Select, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let inner = theme::frame(buf, area, theme, 1, &HINTS);
    if inner.height < 8 {
        return;
    }
    let count = |v: Verdict| state.rows.iter().filter(|r| r.verdict == v).count();
    heading(
        buf,
        inner.x,
        inner.y,
        inner.width,
        theme,
        "tables",
        &format!("{} of {} selected", state.chosen.len(), state.rows.len()),
    );
    // Summary line + selection meter (dot ramp: ● chosen, · not).
    let meter_w = usize::from(inner.width)
        .saturating_sub(48)
        .min(state.rows.len())
        .min(40);
    let chosen_in_meter = if state.rows.is_empty() {
        0
    } else {
        (state.chosen.len() * meter_w).div_ceil(state.rows.len())
    };
    put(
        buf,
        inner.x,
        inner.y + 1,
        inner.width,
        vec![
            theme.span("✓ ", Tone::Ok),
            theme.span(
                format!("{} recommend   ", count(Verdict::Recommend)),
                Tone::Dim,
            ),
            theme.span("? ", Tone::Warn),
            theme.span(format!("{} maybe   ", count(Verdict::Maybe)), Tone::Dim),
            theme.span("✗ ", Tone::Faint),
            theme.span(format!("{} skip   ", count(Verdict::Skip)), Tone::Dim),
            theme.span("●".repeat(chosen_in_meter), Tone::Accent),
            theme.span("·".repeat(meter_w - chosen_in_meter), Tone::Faint),
        ],
    );
    let top = inner.y + 3;
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
        .clamp(10, if inner.width >= 100 { 30 } else { 22 });
    let mut y = top;
    for (pos, &i) in visible.iter().enumerate().skip(scroll).take(list_h) {
        let row = &state.rows[i];
        let on = pos == cursor;
        let picked = state.chosen.contains(&row.table);
        let (glyph, tone) = match row.verdict {
            Verdict::Recommend => ("✓", Tone::Ok),
            Verdict::Maybe => ("?", Tone::Warn),
            Verdict::Skip => ("✗", Tone::Faint),
        };
        let name_tone = if row.verdict == Verdict::Skip && !picked {
            Tone::Dim
        } else {
            Tone::Ink
        };
        let rows = row.rows.map_or("—".to_owned(), magnitude);
        let reason_w = usize::from(inner.width).saturating_sub(name_w + 18);
        let mut spans = vec![
            theme.span(if on { "▸ " } else { "  " }, Tone::Accent),
            theme.span(
                if picked { "◉ " } else { "○ " },
                if picked { Tone::Accent } else { Tone::Faint },
            ),
            theme.span(format!("{glyph} "), tone),
            theme.span(theme::pad(&row.table, name_w), name_tone),
            theme.span(format!("{rows:>6}  "), Tone::Dim),
            theme.span(fit(&row.reason.text(state.zh), reason_w), Tone::Dim),
        ];
        if on {
            spans[3] =
                ratatui::text::Span::styled(theme::pad(&row.table, name_w), theme.selected());
        }
        put(buf, inner.x, y, inner.width, spans);
        y += 1;
    }
    if visible.len() > list_h {
        let more = visible.len() - (scroll + list_h).min(visible.len());
        put(
            buf,
            inner.x + 4,
            y,
            inner.width,
            vec![theme.span(format!("· · · {more} more below"), Tone::Faint)],
        );
    }
    let foot = inner.bottom() - 1;
    if state.searching || !state.search.is_empty() {
        put(buf, inner.x, foot, 8, vec![theme.span("search", Tone::Dim)]);
        input(
            buf,
            inner.x + 7,
            foot,
            inner.width.saturating_sub(7),
            theme,
            &state.search,
            state.searching,
        );
    } else if let Some(error) = &state.error {
        put(
            buf,
            inner.x,
            foot,
            inner.width,
            vec![theme.span(format!("✗ {error}"), Tone::Bad)],
        );
    } else {
        put(
            buf,
            inner.x,
            foot,
            inner.width,
            vec![theme.span(
                "rules + catalog metadata only · no data rows read · names never leave this machine",
                Tone::Faint,
            )],
        );
    }
}

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
        let mut s = Select::new(&demo_tables(), false);
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
        assert!(s.chosen.contains("lg.shipment_log"));
        key(&mut s, KeyEvent::from(KeyCode::Char('n')));
        assert_eq!(key(&mut s, KeyEvent::from(KeyCode::Enter)), Action::None);
        assert!(s.error.is_some());
    }

    #[test]
    fn snapshots() {
        let mut state = Select::new(&demo_tables(), false);
        state.cursor = 2;
        snapshot::all_sizes("2-tables", |area, buf, theme| {
            render(&state, area, buf, theme)
        });
        let mut zh = Select::new(&demo_tables(), true);
        zh.searching = true;
        zh.search = "ship".into();
        snapshot::check(
            "2-tables-zh-search",
            80,
            24,
            Theme::MONO,
            |area, buf, theme| render(&zh, area, buf, theme),
        );
    }
}
