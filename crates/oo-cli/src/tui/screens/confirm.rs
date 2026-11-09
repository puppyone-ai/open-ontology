//! Screen 4: look at the ontology, check the product's evidence-based
//! decisions, override any of them, and confirm once. The confirmation is
//! bound to the summary hash recomputed after overrides (D28).
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Span;
use serde_json::Value;

use super::{Action, blit, input, modal, put};
use crate::tui::adjudication::{
    self, Adjudication, Item, Override, Overrides, evidence_line, flip, magnitude, number,
};
use crate::tui::graph::{self, FOCUS_THRESHOLD, View};
use crate::tui::theme::{self, Theme, Tone, fit, heading, width};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Graph,
    Relations,
    Entity,
    Decisions,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Modal {
    Override {
        item: String,
        to: String,
        reason: String,
        error: Option<String>,
    },
    Confirm {
        reason: String,
        error: Option<String>,
    },
}

#[derive(Clone, Debug)]
pub struct Confirm {
    pub base: Adjudication,
    pub overrides: Overrides,
    pub effective: Adjudication,
    pub tab: Tab,
    pub selected: String,
    pub focus: Option<String>,
    pub relation: usize,
    pub decision: usize,
    pub modal: Option<Modal>,
    /// Width of the last render; decides the wide (side panel) layout.
    pub width: u16,
    /// `confirmed_at` stamp; injected so tests are reproducible.
    pub now: String,
    /// Set when the user confirmed: the publish-confirm document.
    pub confirmed: Option<Value>,
}

pub const PANEL_MIN_WIDTH: u16 = 110;
const PANEL_W: u16 = 46;

impl Confirm {
    pub fn new(base: Adjudication, now: &str) -> Self {
        let order = graph::focus_order(&base);
        let selected = order.first().cloned().unwrap_or_default();
        let focus = (base.graph.entities.len() > FOCUS_THRESHOLD).then(|| selected.clone());
        Confirm {
            overrides: Overrides::new(&base),
            effective: base.clone(),
            base,
            tab: Tab::Graph,
            selected,
            focus,
            relation: 0,
            decision: 0,
            modal: None,
            width: 120,
            now: now.into(),
            confirmed: None,
        }
    }

    fn wide(&self) -> bool {
        self.width >= PANEL_MIN_WIDTH
    }

    /// Decision items, most in need of a look first.
    pub fn ordered(&self) -> Vec<&Item> {
        let rank = |i: &Item| {
            let class = match i.class.as_str() {
                "sensitive" => 0,
                "relation" => 1,
                "metric" => 2,
                "code_table" => 3,
                _ => 4,
            };
            let keep = adjudication::decisions(&i.class).is_some_and(|(a, _)| a == i.decision);
            let changed = self.overrides.items.contains_key(&i.id);
            (!changed, keep && i.class != "sensitive", class)
        };
        let mut items: Vec<&Item> = self.effective.items.iter().collect();
        items.sort_by(|a, b| {
            rank(a).cmp(&rank(b)).then(
                confidence(a)
                    .partial_cmp(&confidence(b))
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
        });
        items
    }

    fn view(&self, max_width: usize) -> View<'_> {
        View {
            selected: Some(self.selected.as_str()),
            focus: self.focus.as_deref(),
            max_width: Some(max_width),
            legend: false,
        }
    }

    fn open_override(&mut self, id: &str) {
        if let Some(item) = self.effective.items.iter().find(|i| i.id == id)
            && let Some(to) = flip(&item.class, &item.decision)
        {
            self.modal = Some(Modal::Override {
                item: id.into(),
                to: to.into(),
                reason: String::new(),
                error: None,
            });
        }
    }

    fn apply_override(&mut self, id: &str, to: &str, reason: &str) -> Result<(), &'static str> {
        let mut next = self.overrides.clone();
        let original = self
            .base
            .items
            .iter()
            .find(|i| i.id == id)
            .map(|i| i.decision.as_str());
        if original == Some(to) {
            next.items.remove(id);
        } else {
            next.items.insert(
                id.into(),
                Override {
                    decision: to.into(),
                    reason: reason.trim().into(),
                },
            );
        }
        self.effective = adjudication::apply(&self.base, &next)?;
        self.overrides = next;
        Ok(())
    }
}

fn confidence(item: &Item) -> f64 {
    item.scores
        .get("confidence")
        .and_then(Value::as_f64)
        .unwrap_or(1.0)
}

fn label<'a>(adj: &'a Adjudication, id: &'a str) -> &'a str {
    adj.graph
        .entities
        .iter()
        .find(|e| e.id == id)
        .map_or(id, |e| e.label.as_str())
}

/// Short subject text for an item.
pub fn subject(adj: &Adjudication, item: &Item) -> String {
    if item.class == "relation"
        && let Some(edge) = adj.graph.edges.iter().find(|e| e.relation == item.id)
    {
        return format!("{} → {}", label(adj, &edge.from), label(adj, &edge.to));
    }
    let text = ["column", "metric", "table", "from"]
        .iter()
        .find_map(|k| item.subject.get(*k).and_then(Value::as_str))
        .unwrap_or(item.id.as_str());
    let parts: Vec<&str> = text.split('.').collect();
    if parts.len() >= 3 {
        parts[parts.len() - 2..].join(".")
    } else {
        text.into()
    }
}

fn decision_glyph(decision: &str) -> (&'static str, Tone) {
    match decision {
        "adopted" | "certified" | "complete" => ("✓", Tone::Ok),
        "rejected" | "incomplete" => ("✗", Tone::Bad),
        "demoted" => ("↓", Tone::Warn),
        "hidden" => ("◌", Tone::Accent),
        _ => ("○", Tone::Dim),
    }
}

pub fn key(state: &mut Confirm, key: KeyEvent) -> Action {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Action::Quit;
    }
    if let Some(m) = state.modal.clone() {
        return modal_key(state, m, key);
    }
    let tabs: &[Tab] = if state.wide() {
        &[Tab::Graph, Tab::Relations, Tab::Entity]
    } else {
        &[Tab::Graph, Tab::Relations, Tab::Entity, Tab::Decisions]
    };
    let at = tabs.iter().position(|t| *t == state.tab).unwrap_or(0);
    let items: Vec<String> = state.ordered().iter().map(|i| i.id.clone()).collect();
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => return Action::Quit,
        KeyCode::Tab => state.tab = tabs[(at + 1) % tabs.len()],
        KeyCode::BackTab => state.tab = tabs[(at + tabs.len() - 1) % tabs.len()],
        KeyCode::Char('g' | '1') => state.tab = Tab::Graph,
        KeyCode::Char('r' | 't' | '2') => state.tab = Tab::Relations,
        KeyCode::Char('e' | '3') => state.tab = Tab::Entity,
        KeyCode::Char('d' | '4') if !state.wide() => state.tab = Tab::Decisions,
        KeyCode::Enter => {
            state.modal = Some(Modal::Confirm {
                reason: String::new(),
                error: None,
            })
        }
        KeyCode::Char('j') => {
            state.decision = (state.decision + 1).min(items.len().saturating_sub(1))
        }
        KeyCode::Char('k') => state.decision = state.decision.saturating_sub(1),
        KeyCode::Char('o') => {
            let id = if state.tab == Tab::Relations {
                state
                    .effective
                    .graph
                    .edges
                    .get(state.relation)
                    .map(|e| e.relation.clone())
            } else {
                items.get(state.decision).cloned()
            };
            if let Some(id) = id {
                state.open_override(&id);
            }
        }
        KeyCode::Char('f') if state.tab == Tab::Graph => {
            if state.focus.is_some() && state.base.graph.entities.len() <= FOCUS_THRESHOLD {
                state.focus = None;
            } else {
                state.focus = Some(state.selected.clone());
            }
        }
        code @ (KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right) => match state.tab {
            Tab::Graph => move_selection(state, code),
            Tab::Relations => {
                let n = state.effective.graph.edges.len();
                state.relation = step(state.relation, code, n);
            }
            Tab::Decisions => state.decision = step(state.decision, code, items.len()),
            Tab::Entity => {
                let order = graph::focus_order(&state.effective);
                if let Some(pos) = order.iter().position(|e| *e == state.selected) {
                    state.selected = order[step(pos, code, order.len())].clone();
                }
            }
        },
        _ => (),
    }
    Action::None
}

fn step(pos: usize, code: KeyCode, n: usize) -> usize {
    match code {
        KeyCode::Up | KeyCode::Left => pos.saturating_sub(1),
        _ => (pos + 1).min(n.saturating_sub(1)),
    }
}

/// Arrow keys pick the nearest box in that direction; in focus mode the focus
/// follows, so arrows walk the graph one hop at a time.
fn move_selection(state: &mut Confirm, code: KeyCode) {
    let main_w = main_width(state.width, state.wide());
    let canvas = graph::render(&state.effective, &state.view(usize::from(main_w)));
    let Some(me) = canvas.boxes.iter().find(|b| b.0 == state.selected) else {
        if let Some(first) = canvas.boxes.first() {
            state.selected = first.0.clone();
        }
        return;
    };
    let (mx, my) = ((me.1 + me.3 / 2) as i64, (me.2 + me.4 / 2) as i64);
    let best = canvas
        .boxes
        .iter()
        .filter(|b| b.0 != me.0)
        .filter_map(|b| {
            let (x, y) = ((b.1 + b.3 / 2) as i64, (b.2 + b.4 / 2) as i64);
            let (dx, dy) = (x - mx, y - my);
            let ok = match code {
                KeyCode::Left => dx < 0 && dx.abs() >= dy.abs() / 2,
                KeyCode::Right => dx > 0 && dx.abs() >= dy.abs() / 2,
                KeyCode::Up => dy < 0,
                _ => dy > 0,
            };
            ok.then_some((dx.abs() + 2 * dy.abs(), b.0.clone()))
        })
        .min();
    if let Some((_, id)) = best {
        state.selected = id;
        if state.focus.is_some() {
            state.focus = Some(state.selected.clone());
        }
    }
}

fn modal_key(state: &mut Confirm, m: Modal, key: KeyEvent) -> Action {
    let typed = |reason: &mut String| match key.code {
        KeyCode::Backspace => {
            reason.pop();
        }
        KeyCode::Char(c) if !c.is_control() && reason.chars().count() < 200 => reason.push(c),
        _ => (),
    };
    match m {
        Modal::Override {
            item,
            to,
            mut reason,
            ..
        } => match key.code {
            KeyCode::Esc => state.modal = None,
            KeyCode::Enter => match state.apply_override(&item, &to, &reason) {
                Ok(()) => state.modal = None,
                Err(e) => {
                    state.modal = Some(Modal::Override {
                        item,
                        to,
                        reason,
                        error: Some(if e == "OVERRIDE_REASON_REQUIRED" {
                            "showing a column the product hid needs a reason".into()
                        } else {
                            e.into()
                        }),
                    })
                }
            },
            _ => {
                typed(&mut reason);
                state.modal = Some(Modal::Override {
                    item,
                    to,
                    reason,
                    error: None,
                });
            }
        },
        Modal::Confirm { mut reason, .. } => match key.code {
            KeyCode::Esc => state.modal = None,
            KeyCode::Enter => {
                let overrides = (!state.overrides.items.is_empty()).then_some(&state.overrides);
                match adjudication::confirm(
                    &state.base,
                    overrides,
                    &state.effective.summary_sha256,
                    &reason,
                    &state.now,
                ) {
                    Ok(doc) => {
                        state.confirmed = Some(doc);
                        state.modal = None;
                        return Action::Next;
                    }
                    Err(e) => {
                        state.modal = Some(Modal::Confirm {
                            reason,
                            error: Some(if e == "CONFIRM_REASON_REQUIRED" {
                                "a short reason is required".into()
                            } else {
                                e.into()
                            }),
                        })
                    }
                }
            }
            _ => {
                typed(&mut reason);
                state.modal = Some(Modal::Confirm {
                    reason,
                    error: None,
                });
            }
        },
    }
    Action::None
}

fn main_width(width: u16, wide: bool) -> u16 {
    let inner = width.saturating_sub(4);
    if wide {
        inner.saturating_sub(PANEL_W + 2)
    } else {
        inner
    }
}

pub fn hints(state: &Confirm) -> Vec<(&'static str, &'static str)> {
    let mut h = vec![("tab", "view")];
    match state.tab {
        Tab::Graph => {
            h.push((
                "←↑↓→",
                if state.focus.is_some() {
                    "move focus"
                } else {
                    "select"
                },
            ));
            if state.base.graph.entities.len() <= FOCUS_THRESHOLD {
                h.push(("f", "focus"));
            }
        }
        Tab::Relations | Tab::Decisions => h.push(("↑↓", "select")),
        Tab::Entity => h.push(("←→", "entity")),
    }
    if state.wide() {
        h.push(("j/k", "decisions"));
    }
    h.push(("o", "override"));
    h.push(("⏎", "confirm publish"));
    h.push(("q", "quit"));
    h
}

pub fn render(state: &Confirm, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let wide = area.width >= PANEL_MIN_WIDTH;
    let mut state_w = state.clone();
    state_w.width = area.width;
    let inner = theme::frame(buf, area, theme, 3, &hints(&state_w));
    if inner.height < 8 {
        return;
    }
    let main_w = if wide {
        inner.width - PANEL_W - 2
    } else {
        inner.width
    };
    // Tabs.
    let mut tabs = vec![
        (Tab::Graph, "graph"),
        (Tab::Relations, "relations"),
        (Tab::Entity, "entity"),
    ];
    if !wide {
        tabs.push((Tab::Decisions, "decisions"));
    }
    let mut spans = Vec::new();
    for (k, (tab, name)) in tabs.iter().enumerate() {
        if k > 0 {
            spans.push(theme.span(" │ ", Tone::Faint));
        }
        if *tab == state.tab {
            spans.push(Span::styled(format!(" {name} "), theme.selected()));
        } else {
            spans.push(theme.span(format!(" {name} "), Tone::Dim));
        }
    }
    let changes = state.overrides.items.len();
    let short = &state.effective.summary_sha256[..8.min(state.effective.summary_sha256.len())];
    let tail = format!(
        "summary {short}…{}",
        if changes > 0 {
            format!(" · {changes} override(s)")
        } else {
            String::new()
        }
    );
    let used: usize = spans.iter().map(Span::width).sum();
    let pad = usize::from(main_w).saturating_sub(used + width(&tail));
    spans.push(theme.span(" ".repeat(pad), Tone::Faint));
    spans.push(theme.span(
        tail,
        if changes > 0 {
            Tone::Accent
        } else {
            Tone::Faint
        },
    ));
    put(buf, inner.x, inner.y, main_w, spans);
    let body = Rect::new(inner.x, inner.y + 2, main_w, inner.height - 2);
    match state.tab {
        Tab::Graph => render_graph(state, body, buf, theme),
        Tab::Relations => render_relations(state, body, buf, theme),
        Tab::Entity => render_entity(state, body, buf, theme),
        Tab::Decisions => render_decisions(state, body, buf, theme, false),
    }
    if wide {
        let px = inner.x + main_w + 1;
        for y in inner.y..inner.bottom() {
            buf[(px, y)]
                .set_symbol("┊")
                .set_style(theme.style(Tone::Faint));
        }
        render_decisions(
            state,
            Rect::new(px + 2, inner.y, PANEL_W - 1, inner.height),
            buf,
            theme,
            true,
        );
    }
    match &state.modal {
        Some(Modal::Override {
            item,
            to,
            reason,
            error,
        }) => {
            let it = state.effective.items.iter().find(|i| &i.id == item);
            let inner = modal(buf, area, theme, "override", 64, 10);
            if let Some(it) = it {
                let (g1, t1) = decision_glyph(&it.decision);
                let (g2, t2) = decision_glyph(to);
                put(
                    buf,
                    inner.x,
                    inner.y,
                    inner.width,
                    vec![
                        theme.span(format!("{} ", it.class), Tone::Dim),
                        theme.span(subject(&state.effective, it), Tone::Ink),
                    ],
                );
                put(
                    buf,
                    inner.x,
                    inner.y + 2,
                    inner.width,
                    vec![
                        theme.span(format!("{g1} {}", it.decision), t1),
                        theme.span("  →  ", Tone::Faint),
                        theme.span(format!("{g2} {to}"), t2),
                    ],
                );
                put(
                    buf,
                    inner.x,
                    inner.y + 3,
                    inner.width,
                    vec![theme.span(fit(&it.reason, usize::from(inner.width)), Tone::Faint)],
                );
            }
            put(
                buf,
                inner.x,
                inner.y + 5,
                8,
                vec![theme.span("reason", Tone::Dim)],
            );
            input(
                buf,
                inner.x + 7,
                inner.y + 5,
                inner.width - 7,
                theme,
                reason,
                true,
            );
            if let Some(e) = error {
                put(
                    buf,
                    inner.x,
                    inner.y + 6,
                    inner.width,
                    vec![theme.span(format!("✗ {e}"), Tone::Bad)],
                );
            }
            put(
                buf,
                inner.x,
                inner.y + 7,
                inner.width,
                vec![
                    theme.span("⏎", Tone::Accent),
                    theme.span(" apply   ", Tone::Dim),
                    theme.span("esc", Tone::Accent),
                    theme.span(" cancel", Tone::Dim),
                ],
            );
        }
        Some(Modal::Confirm { reason, error }) => {
            let inner = modal(buf, area, theme, "confirm publication", 66, 12);
            let c = adjudication::counts(&state.effective);
            let h = &state.effective.summary_sha256;
            put(
                buf,
                inner.x,
                inner.y,
                inner.width,
                vec![
                    theme.span("summary  ", Tone::Dim),
                    theme.span(
                        format!(
                            "sha256:{}…{}",
                            &h[..12.min(h.len())],
                            &h[h.len().saturating_sub(6)..]
                        ),
                        Tone::Accent,
                    ),
                ],
            );
            put(
                buf,
                inner.x,
                inner.y + 1,
                inner.width,
                vec![theme.span(
                    format!(
                        "{} entities · {} relations · {} rejected",
                        c["entities"], c["relations"], c["rejected_relations"]
                    ),
                    Tone::Ink,
                )],
            );
            put(
                buf,
                inner.x,
                inner.y + 2,
                inner.width,
                vec![theme.span(
                    format!(
                        "{} hidden columns · {} demoted metrics · {} override(s)",
                        c["hidden_columns"],
                        c["demoted_metrics"],
                        state.overrides.items.len()
                    ),
                    Tone::Ink,
                )],
            );
            put(
                buf,
                inner.x,
                inner.y + 4,
                inner.width,
                vec![theme.span(
                    fit(
                        "Publishing binds this exact graph and these decisions.",
                        usize::from(inner.width),
                    ),
                    Tone::Faint,
                )],
            );
            put(
                buf,
                inner.x,
                inner.y + 6,
                8,
                vec![theme.span("reason", Tone::Dim)],
            );
            input(
                buf,
                inner.x + 7,
                inner.y + 6,
                inner.width - 7,
                theme,
                reason,
                true,
            );
            if let Some(e) = error {
                put(
                    buf,
                    inner.x,
                    inner.y + 7,
                    inner.width,
                    vec![theme.span(format!("✗ {e}"), Tone::Bad)],
                );
            }
            put(
                buf,
                inner.x,
                inner.y + 9,
                inner.width,
                vec![
                    theme.span("⏎", Tone::Accent),
                    theme.span(" confirm and write   ", Tone::Dim),
                    theme.span("esc", Tone::Accent),
                    theme.span(" back", Tone::Dim),
                ],
            );
        }
        None => (),
    }
}

fn render_graph(state: &Confirm, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let canvas = graph::render(&state.effective, &state.view(usize::from(area.width)));
    let view_h = usize::from(area.height.saturating_sub(2));
    let view_w = usize::from(area.width);
    let (mut ox, mut oy) = (0usize, 0usize);
    if let Some(b) = canvas.boxes.iter().find(|b| b.0 == state.selected) {
        if canvas.width > view_w {
            ox = (b.1 + b.3 / 2)
                .saturating_sub(view_w / 2)
                .min(canvas.width - view_w);
        }
        if canvas.height > view_h {
            oy = (b.2 + b.4 / 2)
                .saturating_sub(view_h / 2)
                .min(canvas.height - view_h);
        }
    }
    let left = view_w.saturating_sub(canvas.width) / 2;
    blit(
        buf,
        Rect::new(
            area.x + left as u16,
            area.y,
            area.width - left as u16,
            view_h as u16,
        ),
        theme,
        &canvas.cells,
        (ox, oy),
    );
    let total = state.effective.graph.entities.len();
    let status = match &canvas.focus {
        Some(f) => format!(
            "focus {} · {} of {total} entities · one hop",
            label(&state.effective, f),
            canvas.shown.len()
        ),
        None => format!(
            "{total} entities · {} relations",
            state.effective.graph.edges.len()
        ),
    };
    let more = if canvas.height > view_h || canvas.width > view_w {
        " ↕"
    } else {
        ""
    };
    let mut spans = vec![
        theme.span("N─1 ", Tone::Accent),
        theme.span("many-to-one  ", Tone::Dim),
        theme.span("┄ ", Tone::Bad),
        theme.span("rejected  ", Tone::Dim),
    ];
    if area.width >= 90 {
        spans.push(theme.span("━ ", Tone::Accent));
        spans.push(theme.span("selected  ", Tone::Dim));
    }
    spans.push(theme.span(format!(" {status}{more}"), Tone::Faint));
    put(buf, area.x, area.bottom() - 1, area.width, spans);
}

fn render_relations(state: &Confirm, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let rels = graph::relations(&state.effective);
    let from_w = rels
        .iter()
        .map(|r| width(&r.from))
        .max()
        .unwrap_or(0)
        .min(14);
    let to_w = rels.iter().map(|r| width(&r.to)).max().unwrap_or(0).min(14);
    let via_w = rels
        .iter()
        .map(|r| width(&r.via))
        .max()
        .unwrap_or(0)
        .min(if area.width >= 90 { 32 } else { 22 })
        + 4;
    let h = usize::from(area.height);
    let scroll = state.relation.saturating_sub(h.saturating_sub(1));
    for (k, r) in rels.iter().enumerate().skip(scroll).take(h) {
        let y = area.y + (k - scroll) as u16;
        let on = k == state.relation;
        let overridden = state.overrides.items.contains_key(&r.relation);
        let status = if r.adopted {
            format!("✓ {}", r.confidence)
        } else {
            format!("✗ {}", r.note)
        };
        let used = 2 + from_w + 5 + to_w + 2 + via_w;
        let mut spans = vec![
            theme.span(if on { "▸ " } else { "  " }, Tone::Accent),
            theme.span(theme::pad(&r.from, from_w), Tone::Ink),
            theme.span(
                format!(" {} ", r.arrow),
                if r.adopted { Tone::Info } else { Tone::Bad },
            ),
            theme.span(theme::pad(&r.to, to_w), Tone::Ink),
            theme.span(
                format!("  {} ", theme::pad(&format!("via {}", r.via), via_w - 1)),
                Tone::Dim,
            ),
            theme.span(
                fit(
                    &format!("{}{status}", if overridden { "✎ " } else { "" }),
                    usize::from(area.width).saturating_sub(used),
                ),
                if overridden {
                    Tone::Accent
                } else if r.adopted {
                    Tone::Ok
                } else {
                    Tone::Bad
                },
            ),
        ];
        if on {
            spans[1] = Span::styled(theme::pad(&r.from, from_w), theme.selected());
        }
        put(buf, area.x, y, area.width, spans);
    }
}

fn render_entity(state: &Confirm, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let adj = &state.effective;
    let Some(e) = adj.graph.entities.iter().find(|e| e.id == state.selected) else {
        return;
    };
    heading(buf, area.x, area.y, area.width, theme, &e.label, &e.table);
    let rows = [
        ("key", e.key.join(", ")),
        ("display", e.display.clone().unwrap_or_else(|| "—".into())),
        (
            "rows",
            e.rows
                .map_or("unknown".into(), |r| format!("{} ({r})", magnitude(r))),
        ),
    ];
    let mut y = area.y + 2;
    for (k, v) in rows {
        put(
            buf,
            area.x,
            y,
            area.width,
            vec![
                theme.span(theme::pad(k, 10), Tone::Dim),
                theme.span(v, Tone::Ink),
            ],
        );
        y += 1;
    }
    y += 1;
    put(
        buf,
        area.x,
        y,
        area.width,
        vec![theme.span("relations", Tone::Dim)],
    );
    y += 1;
    for r in graph::relations(adj).iter().zip(&adj.graph.edges) {
        let (line, edge) = r;
        if edge.from != e.id && edge.to != e.id {
            continue;
        }
        if y + 1 >= area.bottom() {
            break;
        }
        put(
            buf,
            area.x + 2,
            y,
            area.width - 2,
            vec![
                theme.span(format!("{} ", line.from), Tone::Ink),
                theme.span(
                    line.arrow.clone(),
                    if line.adopted { Tone::Info } else { Tone::Bad },
                ),
                theme.span(format!(" {}", line.to), Tone::Ink),
                theme.span(format!("  via {}", line.via), Tone::Dim),
                theme.span(
                    if line.adopted {
                        "  ✓".to_owned()
                    } else {
                        format!("  ✗ {}", line.note)
                    },
                    if line.adopted { Tone::Ok } else { Tone::Bad },
                ),
            ],
        );
        y += 1;
    }
    let table = e.table.clone();
    let own: Vec<&Item> = adj
        .items
        .iter()
        .filter(|i| i.class != "relation")
        .filter(|i| {
            i.subject.values().any(|v| {
                v.as_str()
                    .is_some_and(|s| s == table || s.starts_with(&format!("{table}.")))
            })
        })
        .collect();
    if !own.is_empty() && y + 2 < area.bottom() {
        y += 1;
        put(
            buf,
            area.x,
            y,
            area.width,
            vec![theme.span("decisions on this table", Tone::Dim)],
        );
        y += 1;
        for item in own {
            if y >= area.bottom() {
                break;
            }
            let (g, t) = decision_glyph(&item.decision);
            put(
                buf,
                area.x + 2,
                y,
                area.width - 2,
                vec![
                    theme.span(format!("{g} "), t),
                    theme.span(theme::pad(&item.class, 11), Tone::Dim),
                    theme.span(theme::pad(&subject(adj, item), 22), Tone::Ink),
                    theme.span(
                        fit(&item.reason, usize::from(area.width).saturating_sub(40)),
                        Tone::Faint,
                    ),
                ],
            );
            y += 1;
        }
    }
}

fn render_decisions(state: &Confirm, area: Rect, buf: &mut Buffer, theme: &Theme, panel: bool) {
    let items = state.ordered();
    heading(
        buf,
        area.x,
        area.y,
        area.width,
        theme,
        "decisions",
        "by data evidence",
    );
    let per = if area.height >= 30 { 3 } else { 2 };
    let top = area.y + 2;
    let slots = usize::from(area.bottom().saturating_sub(top)) / per;
    let cursor = state.decision.min(items.len().saturating_sub(1));
    let scroll = cursor.saturating_sub(slots.saturating_sub(1));
    for (k, item) in items.iter().enumerate().skip(scroll).take(slots) {
        let y = top + ((k - scroll) * per) as u16;
        let on = k == cursor;
        let (g, t) = decision_glyph(&item.decision);
        let changed = state.overrides.items.contains_key(&item.id);
        let score = item
            .scores
            .get("confidence")
            .map(number)
            .unwrap_or_default();
        let class = match item.class.as_str() {
            "entity_key" => "key",
            "code_table" => "codes",
            other => other,
        };
        let w = usize::from(area.width);
        let subj_w = w.saturating_sub(2 + 2 + 10 + 11 + 5);
        let mut spans = vec![
            theme.span(if on { "▸ " } else { "  " }, Tone::Accent),
            theme.span(format!("{g} "), t),
            theme.span(theme::pad(class, 10), Tone::Dim),
            theme.span(
                format!(
                    "{} ",
                    theme::pad(&subject(&state.effective, item), subj_w - 1)
                ),
                Tone::Ink,
            ),
            theme.span(
                theme::pad(
                    &format!("{}{}", if changed { "✎" } else { "" }, item.decision),
                    11,
                ),
                if changed { Tone::Accent } else { t },
            ),
            theme.span(format!("{score:>4}"), Tone::Dim),
        ];
        if on && (panel || state.tab == Tab::Decisions) {
            spans[3] = Span::styled(
                format!(
                    "{} ",
                    theme::pad(&subject(&state.effective, item), subj_w - 1)
                ),
                theme.selected(),
            );
        }
        put(buf, area.x, y, area.width, spans);
        let evidence = if matches!(item.decision.as_str(), "rejected") {
            adjudication::missing_evidence(item, &state.effective.thresholds)
        } else {
            evidence_line(item)
        };
        put(
            buf,
            area.x + 4,
            y + 1,
            area.width.saturating_sub(4),
            vec![theme.span(fit(&evidence, w.saturating_sub(4)), Tone::Faint)],
        );
        if per == 3 {
            put(
                buf,
                area.x + 4,
                y + 2,
                area.width.saturating_sub(4),
                vec![theme.span(fit(&item.rule_id, w.saturating_sub(4)), Tone::Faint)],
            );
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::snapshot;
    use super::*;
    use crate::tui::adjudication::fixture;

    fn press(state: &mut Confirm, codes: &[KeyCode]) -> Action {
        let mut last = Action::None;
        for code in codes {
            last = key(state, KeyEvent::from(*code));
        }
        last
    }

    fn typed(state: &mut Confirm, text: &str) {
        for c in text.chars() {
            key(state, KeyEvent::from(KeyCode::Char(c)));
        }
    }

    #[test]
    fn override_then_confirm_binds_the_effective_summary() {
        let mut s = Confirm::new(fixture(), "2026-11-09T00:00:00Z");
        s.tab = Tab::Relations;
        let rejected = s
            .effective
            .graph
            .edges
            .iter()
            .position(|e| !e.adopted)
            .unwrap();
        s.relation = rejected;
        press(&mut s, &[KeyCode::Char('o')]);
        typed(&mut s, "known FK");
        press(&mut s, &[KeyCode::Enter]);
        assert_eq!(s.overrides.items.len(), 1);
        assert_ne!(s.effective.summary_sha256, s.base.summary_sha256);
        assert_eq!(
            press(&mut s, &[KeyCode::Enter, KeyCode::Enter]),
            Action::None,
            "empty reason refused"
        );
        typed(&mut s, "first publish");
        assert_eq!(press(&mut s, &[KeyCode::Enter]), Action::Next);
        let doc = s.confirmed.unwrap();
        assert_eq!(
            doc["summary_sha256"],
            serde_json::json!(s.effective.summary_sha256)
        );
        assert_eq!(
            doc["overrides_sha256"],
            serde_json::json!(adjudication::overrides_sha256(&s.overrides))
        );
    }

    #[test]
    fn overriding_back_removes_the_override_and_unhiding_needs_reason() {
        let mut s = Confirm::new(fixture(), "t");
        s.width = 80;
        s.tab = Tab::Decisions;
        // Sensitive columns are listed first.
        assert_eq!(s.ordered()[0].class, "sensitive");
        s.decision = s
            .ordered()
            .iter()
            .position(|i| i.decision == "hidden")
            .unwrap();
        let id = s.ordered()[s.decision].id.clone();
        press(&mut s, &[KeyCode::Char('o'), KeyCode::Enter]);
        assert!(matches!(
            s.modal,
            Some(Modal::Override { error: Some(_), .. })
        ));
        typed(&mut s, "public office number");
        press(&mut s, &[KeyCode::Enter]);
        assert_eq!(s.overrides.items[&id].decision, "visible");
        let pos = s.ordered().iter().position(|i| i.id == id).unwrap();
        s.decision = pos;
        press(&mut s, &[KeyCode::Char('o'), KeyCode::Enter]);
        assert!(s.overrides.items.is_empty());
        assert_eq!(s.effective.summary_sha256, s.base.summary_sha256);
    }

    #[test]
    fn arrows_walk_the_graph_and_focus_follows_in_large_graphs() {
        let mut s = Confirm::new(fixture(), "t");
        let start = s.selected.clone();
        press(&mut s, &[KeyCode::Down]);
        assert_ne!(s.selected, start);
        let edges: Vec<(usize, usize, bool)> = (1..30).map(|i| (i, (i - 1) / 3, true)).collect();
        let big = crate::tui::graph::tests::synthetic(30, &edges);
        let mut s = Confirm::new(big, "t");
        assert!(s.focus.is_some());
        let before = s.focus.clone();
        press(&mut s, &[KeyCode::Down]);
        assert_ne!(s.focus, before);
        assert_eq!(s.focus.as_deref(), Some(s.selected.as_str()));
    }

    #[test]
    fn snapshots() {
        let s = Confirm::new(fixture(), "2026-11-09T00:00:00Z");
        snapshot::all_sizes("4-confirm-graph", |area, buf, theme| {
            render(&s, area, buf, theme)
        });
        let mut rel = s.clone();
        rel.tab = Tab::Relations;
        rel.relation = 4;
        snapshot::all_sizes("4-confirm-relations", |area, buf, theme| {
            render(&rel, area, buf, theme)
        });
        let mut ent = s.clone();
        ent.tab = Tab::Entity;
        ent.selected = "entity/customers".into();
        snapshot::all_sizes("4-confirm-entity", |area, buf, theme| {
            render(&ent, area, buf, theme)
        });
        let mut dec = s.clone();
        dec.tab = Tab::Decisions;
        dec.width = 80;
        snapshot::check(
            "4-confirm-decisions",
            80,
            24,
            Theme::MONO,
            |area, buf, theme| render(&dec, area, buf, theme),
        );
        snapshot::check(
            "4-confirm-decisions",
            80,
            24,
            Theme::COLOR,
            |area, buf, theme| render(&dec, area, buf, theme),
        );
        let mut m = s.clone();
        m.modal = Some(Modal::Confirm {
            reason: "first publish of logistics".into(),
            error: None,
        });
        snapshot::all_sizes("4-confirm-modal", |area, buf, theme| {
            render(&m, area, buf, theme)
        });
        let edges: Vec<(usize, usize, bool)> =
            (1..30).map(|i| (i, (i - 1) / 4, i % 9 != 0)).collect();
        let big = Confirm::new(crate::tui::graph::tests::synthetic(30, &edges), "t");
        snapshot::all_sizes("4-confirm-focus", |area, buf, theme| {
            render(&big, area, buf, theme)
        });
    }
}
