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
    self, ACCEPT_INCOMPLETE, Adjudication, DISCOVERY_OVERRIDE, Item, Override, Overrides,
    evidence_line, flip, magnitude, number,
};
use crate::tui::fx;
#[cfg(test)]
use crate::tui::graph::REVEAL_FRAMES;
use crate::tui::graph::{self, FOCUS_THRESHOLD, View};
use crate::tui::theme::{self, Hint, Theme, Tone, fit, heading, width};

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
    /// Explicit acceptance of an incomplete relationship discovery.
    AcceptIncomplete {
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
    /// Animation frame for the graph reveal; the runner advances it.
    pub frame: usize,
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
            frame: 0,
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
            // A fan-out JOIN cannot be adopted; say why up front.
            let error = (to == "adopted" && adjudication::fans_out(item)).then(|| fanout_why(item));
            self.modal = Some(Modal::Override {
                item: id.into(),
                to: to.into(),
                reason: String::new(),
                error,
            });
        }
    }

    /// Relations left unchecked by an incomplete discovery, if any.
    pub fn unchecked(&self) -> Option<usize> {
        self.base.unchecked_relations()
    }

    /// Incomplete discovery that the owner has not accepted yet.
    pub fn needs_acceptance(&self) -> bool {
        self.unchecked().is_some() && !self.overrides.accepts_incomplete()
    }

    fn accept_incomplete(&mut self, reason: &str) -> Result<(), &'static str> {
        let mut next = self.overrides.clone();
        next.items.insert(
            DISCOVERY_OVERRIDE.into(),
            Override {
                decision: ACCEPT_INCOMPLETE.into(),
                reason: reason.trim().into(),
            },
        );
        self.effective = adjudication::apply(&self.base, &next)?;
        self.overrides = next;
        Ok(())
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

/// Why a relation cannot be adopted: a JOIN on it would multiply rows.
fn fanout_why(item: &Item) -> String {
    let scope = adjudication::scope_columns(item);
    let fanout = item
        .evidence
        .get("max_fanout")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let mut why = String::from("parent key repeats");
    if fanout > 1 {
        why.push_str(&format!(" (fan-out ×{fanout})"));
    }
    if scope.is_empty() {
        why.push_str("; a JOIN would multiply rows");
    } else {
        why.push_str(&format!("; filter on {} first", scope.join(", ")));
    }
    why
}

/// Error text for a modal, in the interface language.
fn error_text(e: &str, theme: &Theme) -> String {
    if !theme.zh {
        return e.to_owned();
    }
    if let Some(rest) = e.strip_prefix("parent key repeats") {
        let fanout = rest
            .split_once("×")
            .and_then(|(_, t)| t.split(')').next())
            .map(|n| format!("（扇出 ×{n}）"))
            .unwrap_or_default();
        return match rest.split_once("filter on ") {
            Some((_, cols)) => format!(
                "父键重复{fanout}；先按 {} 过滤",
                cols.trim_end_matches(" first")
            ),
            None => format!("父键重复{fanout}；JOIN 会让行数翻倍"),
        };
    }
    match e {
        "showing a column the product hid needs a reason" => "显示被隐藏的列需要理由".into(),
        "a short reason is required" => "需要一句简短的理由".into(),
        "relationship discovery is incomplete; accept it first (a)" => {
            "关系发现未完成，请先接受（a）".into()
        }
        other => other.to_owned(),
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

/// Display word for a decision value.
fn decision_word<'a>(decision: &'a str, theme: &Theme) -> &'a str {
    if !theme.zh {
        return decision;
    }
    match decision {
        "adopted" => "采纳",
        "rejected" => "拒绝",
        "hidden" => "隐藏",
        "visible" => "可见",
        "demoted" => "降级",
        "certified" => "认证",
        "complete" => "完整",
        "incomplete" => "不完整",
        other => other,
    }
}

/// Display word for an item class.
fn class_word(class: &str, theme: &Theme) -> &'static str {
    match class {
        "sensitive" => theme.t("sensitive", "敏感"),
        "relation" => theme.t("relation", "关系"),
        "metric" => theme.t("metric", "指标"),
        "entity_key" => theme.t("key", "主键"),
        "code_table" => theme.t("codes", "代码表"),
        _ => theme.t("other", "其他"),
    }
}

/// The reason in the interface language: Chinese copy per rule, else the
/// adjudicator's own sentence.
fn reason_text(item: &Item, theme: &Theme) -> String {
    if !theme.zh {
        return item.reason.clone();
    }
    if item.rule_id == "relation.scope_required" {
        let scope = adjudication::scope_columns(item);
        return format!(
            "父键重复，按 {} 过滤后才唯一；请先过滤父表",
            scope.join("、")
        );
    }
    match item.rule_id.as_str() {
        "relation.declared_fk" => "声明了外键，数据也吻合",
        "relation.containment_strict" => "包含度达标，父键唯一",
        "relation.containment_with_signal" => "包含度达标，且有命名和 JOIN 证据",
        "relation.containment_insufficient" => "包含度不足，未采纳",
        "relation.containment_signal" => "包含度达标，且有命名、JOIN 或外键证据",
        "relation.insufficient" => "证据不足，未采纳",
        "relation.parent_not_unique" => "父键重复，JOIN 会让子表行数翻倍",
        "relation.normalized_collision" => "去空格、忽略大小写后父键冲突，匹配有歧义",
        "relation.no_signal" => "值能对上，但没有命名、声明或查询把这两列联系起来",
        "sensitive.format_phone" => "大部分值符合电话号码格式",
        "sensitive.format_national_id" => "值符合身份证件格式",
        "sensitive.name_only_refuted" => "只是名字像个人信息，值不符合",
        "metric.reconciled" => "对账检查全部通过",
        "metric.unit_mismatch" => "单位混用，降级为示例查询",
        "entity_key.unique_non_null" => "唯一且不为空",
        "entity_key.declared_pk" => "声明的主键，唯一且不为空",
        "entity_key.declared_unique" => "声明的唯一键，唯一且不为空",
        "entity_key.observed_unique" => "实测唯一且不为空",
        "entity_key.no_candidate" => "没有可用的主键候选",
        "entity_key.dataset" => "候选键都不唯一，按整表处理",
        "code_table.coverage" => "代码值基本都在代码表里",
        _ => &item.reason,
    }
    .to_owned()
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
        KeyCode::Enter if state.needs_acceptance() => {
            state.modal = Some(Modal::AcceptIncomplete {
                reason: String::new(),
                error: None,
            })
        }
        KeyCode::Enter => {
            state.modal = Some(Modal::Confirm {
                reason: String::new(),
                error: None,
            })
        }
        KeyCode::Char('a') if state.unchecked().is_some() => {
            state.modal = Some(Modal::AcceptIncomplete {
                reason: state
                    .overrides
                    .items
                    .get(DISCOVERY_OVERRIDE)
                    .map(|o| o.reason.clone())
                    .unwrap_or_default(),
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
                    let error = match e {
                        "OVERRIDE_REASON_REQUIRED" => {
                            "showing a column the product hid needs a reason".into()
                        }
                        "OVERRIDE_FANOUT" => state
                            .base
                            .items
                            .iter()
                            .find(|i| i.id == item)
                            .map_or_else(|| e.into(), fanout_why),
                        _ => e.into(),
                    };
                    state.modal = Some(Modal::Override {
                        item,
                        to,
                        reason,
                        error: Some(error),
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
                            error: Some(match e {
                                "CONFIRM_REASON_REQUIRED" => "a short reason is required".into(),
                                "CONFIRM_DISCOVERY_INCOMPLETE" => {
                                    "relationship discovery is incomplete; accept it first (a)"
                                        .into()
                                }
                                _ => e.into(),
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
        Modal::AcceptIncomplete { mut reason, .. } => match key.code {
            KeyCode::Esc => state.modal = None,
            KeyCode::Enter => match state.accept_incomplete(&reason) {
                Ok(()) => state.modal = None,
                Err(e) => {
                    state.modal = Some(Modal::AcceptIncomplete {
                        reason,
                        error: Some(if e == "OVERRIDE_REASON_REQUIRED" {
                            "a short reason is required".into()
                        } else {
                            e.into()
                        }),
                    })
                }
            },
            _ => {
                typed(&mut reason);
                state.modal = Some(Modal::AcceptIncomplete {
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

pub fn hints(state: &Confirm) -> Vec<Hint> {
    let mut h: Vec<Hint> = vec![("tab", "view", "视图")];
    match state.tab {
        Tab::Graph => {
            h.push(if state.focus.is_some() {
                ("←↑↓→", "move focus", "移动焦点")
            } else {
                ("←↑↓→", "select", "选择")
            });
            if state.base.graph.entities.len() <= FOCUS_THRESHOLD {
                h.push(("f", "focus", "聚焦"));
            }
        }
        Tab::Relations | Tab::Decisions => h.push(("↑↓", "select", "选择")),
        Tab::Entity => h.push(("←→", "entity", "实体")),
    }
    if state.wide() {
        h.push(("j/k", "decisions", "决策"));
    }
    h.push(("o", "override", "改判"));
    if state.unchecked().is_some() {
        h.push(("a", "accept incomplete", "接受未查完"));
    }
    h.push(("⏎", "confirm publish", "确认发布"));
    h.push(("q", "quit", "退出"));
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
        (Tab::Graph, theme.t("graph", "图谱")),
        (Tab::Relations, theme.t("relations", "关系")),
        (Tab::Entity, theme.t("entity", "实体")),
    ];
    if !wide {
        tabs.push((Tab::Decisions, theme.t("decisions", "决策")));
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
    let changes = state.overrides.decision_count();
    let short = &state.effective.summary_sha256[..8.min(state.effective.summary_sha256.len())];
    let tail = format!(
        "{} {short}…{}",
        theme.t("summary", "摘要"),
        match (changes > 0, theme.zh) {
            (false, _) => String::new(),
            (true, false) => format!(" · {changes} override(s)"),
            (true, true) => format!(" · 改判 {changes} 项"),
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
    if let Some(n) = state.unchecked() {
        put(
            buf,
            inner.x,
            inner.y + 1,
            main_w,
            discovery_banner(n, state.overrides.accepts_incomplete(), theme),
        );
    }
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
            let inner = modal(buf, area, theme, theme.t("override", "改判"), 64, 10);
            if let Some(it) = it {
                let (g1, t1) = decision_glyph(&it.decision);
                let (g2, t2) = decision_glyph(to);
                put(
                    buf,
                    inner.x,
                    inner.y,
                    inner.width,
                    vec![
                        theme.span(format!("{} ", class_word(&it.class, theme)), Tone::Dim),
                        theme.span(subject(&state.effective, it), Tone::Ink),
                    ],
                );
                put(
                    buf,
                    inner.x,
                    inner.y + 2,
                    inner.width,
                    vec![
                        theme.span(format!("{g1} {}", decision_word(&it.decision, theme)), t1),
                        theme.span("  →  ", Tone::Faint),
                        theme.span(format!("{g2} {}", decision_word(to, theme)), t2),
                    ],
                );
                put(
                    buf,
                    inner.x,
                    inner.y + 3,
                    inner.width,
                    vec![theme.span(
                        fit(&reason_text(it, theme), usize::from(inner.width)),
                        Tone::Faint,
                    )],
                );
            }
            put(
                buf,
                inner.x,
                inner.y + 5,
                8,
                vec![theme.span(theme.t("reason", "理由"), Tone::Dim)],
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
                    vec![theme.span(
                        fit(
                            &format!("✗ {}", error_text(e, theme)),
                            usize::from(inner.width),
                        ),
                        Tone::Bad,
                    )],
                );
            }
            put(
                buf,
                inner.x,
                inner.y + 7,
                inner.width,
                vec![
                    theme.span("⏎", Tone::Accent),
                    theme.span(theme.t(" apply   ", " 应用   "), Tone::Dim),
                    theme.span("esc", Tone::Accent),
                    theme.span(theme.t(" cancel", " 取消"), Tone::Dim),
                ],
            );
        }
        Some(Modal::Confirm { reason, error }) => {
            let inner = modal(
                buf,
                area,
                theme,
                theme.t("confirm publication", "确认发布"),
                66,
                12,
            );
            let c = adjudication::counts(&state.effective);
            let h = &state.effective.summary_sha256;
            put(
                buf,
                inner.x,
                inner.y,
                inner.width,
                vec![
                    theme.span(theme.t("summary  ", "摘要  "), Tone::Dim),
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
                    if theme.zh {
                        format!(
                            "{} 个实体 · {} 条关系 · 拒绝 {} 条",
                            c["entities"], c["relations"], c["rejected_relations"]
                        )
                    } else {
                        format!(
                            "{} entities · {} relations · {} rejected",
                            c["entities"], c["relations"], c["rejected_relations"]
                        )
                    },
                    Tone::Ink,
                )],
            );
            put(
                buf,
                inner.x,
                inner.y + 2,
                inner.width,
                vec![theme.span(
                    if theme.zh {
                        format!(
                            "隐藏 {} 列 · 降级 {} 个指标 · 改判 {} 项",
                            c["hidden_columns"],
                            c["demoted_metrics"],
                            state.overrides.decision_count()
                        )
                    } else {
                        format!(
                            "{} hidden columns · {} demoted metrics · {} override(s)",
                            c["hidden_columns"],
                            c["demoted_metrics"],
                            state.overrides.decision_count()
                        )
                    },
                    Tone::Ink,
                )],
            );
            if let Some(n) = state.unchecked() {
                let accepted = state.overrides.accepts_incomplete();
                put(
                    buf,
                    inner.x,
                    inner.y + 3,
                    inner.width,
                    vec![theme.span(
                        fit(
                            &match (accepted, theme.zh) {
                                (true, false) => {
                                    format!("incomplete: {n} relations not checked · accepted")
                                }
                                (true, true) => format!("未查完：{n} 条关系未检查 · 已接受"),
                                (false, false) => {
                                    format!("incomplete: {n} relations not checked · press a")
                                }
                                (false, true) => format!("未查完：{n} 条关系未检查 · 按 a 接受"),
                            },
                            usize::from(inner.width),
                        ),
                        if accepted { Tone::Warn } else { Tone::Bad },
                    )],
                );
            }
            put(
                buf,
                inner.x,
                inner.y + 4,
                inner.width,
                vec![theme.span(
                    fit(
                        theme.t(
                            "Publishing binds this exact graph and these decisions.",
                            "发布会绑定这一版图谱和这些决策。",
                        ),
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
                vec![theme.span(theme.t("reason", "理由"), Tone::Dim)],
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
                    vec![theme.span(
                        fit(
                            &format!("✗ {}", error_text(e, theme)),
                            usize::from(inner.width),
                        ),
                        Tone::Bad,
                    )],
                );
            }
            put(
                buf,
                inner.x,
                inner.y + 9,
                inner.width,
                vec![
                    theme.span("⏎", Tone::Accent),
                    theme.span(
                        theme.t(" confirm and write   ", " 确认并写入   "),
                        Tone::Dim,
                    ),
                    theme.span("esc", Tone::Accent),
                    theme.span(theme.t(" back", " 返回"), Tone::Dim),
                ],
            );
        }
        Some(Modal::AcceptIncomplete { reason, error }) => {
            let n = state.unchecked().unwrap_or(0);
            let inner = modal(
                buf,
                area,
                theme,
                theme.t("accept incomplete discovery", "接受未查完的关系发现"),
                66,
                12,
            );
            let lines = if theme.zh {
                vec![
                    format!("还有 {n} 条候选关系没有检查（预算用完）。"),
                    "未检查的关系不会进入图谱；之后可以续查。".to_owned(),
                    "接受后才能确认发布，理由会写进改判记录。".to_owned(),
                ]
            } else {
                vec![
                    format!("{n} candidate relations were not checked (budget spent)."),
                    "Unchecked relations stay out of the graph; resume later.".to_owned(),
                    "Publishing needs this acceptance; the reason is recorded.".to_owned(),
                ]
            };
            for (k, line) in lines.iter().enumerate() {
                put(
                    buf,
                    inner.x,
                    inner.y + k as u16,
                    inner.width,
                    vec![theme.span(
                        fit(line, usize::from(inner.width)),
                        if k == 0 { Tone::Ink } else { Tone::Faint },
                    )],
                );
            }
            if let Some(d) = &state.base.discovery
                && let Some(u) = d.unchecked.first()
            {
                let first = format!(
                    "{}.{} → {}.{}",
                    u.from,
                    u.from_columns.join(","),
                    u.to,
                    u.to_columns.join(",")
                );
                let text = match (d.unchecked.len() - 1, theme.zh) {
                    (0, _) => first,
                    (m, false) => format!("{first} · +{m} more"),
                    (m, true) => format!("{first} · 另 {m} 条"),
                };
                put(
                    buf,
                    inner.x,
                    inner.y + 4,
                    inner.width,
                    vec![theme.span(fit(&text, usize::from(inner.width)), Tone::Dim)],
                );
            }
            put(
                buf,
                inner.x,
                inner.y + 6,
                8,
                vec![theme.span(theme.t("reason", "理由"), Tone::Dim)],
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
                    vec![theme.span(
                        fit(
                            &format!("✗ {}", error_text(e, theme)),
                            usize::from(inner.width),
                        ),
                        Tone::Bad,
                    )],
                );
            }
            put(
                buf,
                inner.x,
                inner.y + 9,
                inner.width,
                vec![
                    theme.span("⏎", Tone::Accent),
                    theme.span(theme.t(" accept   ", " 接受   "), Tone::Dim),
                    theme.span("esc", Tone::Accent),
                    theme.span(theme.t(" back", " 返回"), Tone::Dim),
                ],
            );
        }
        None => (),
    }
}

/// The incomplete-discovery line under the tab bar.
fn discovery_banner(n: usize, accepted: bool, theme: &Theme) -> Vec<Span<'static>> {
    let head = if theme.zh {
        format!("未查完：{n} 条关系未检查")
    } else {
        format!("incomplete: {n} relations not checked")
    };
    if accepted {
        vec![
            theme.span("✓ ", Tone::Ok),
            theme.span(head, Tone::Warn),
            theme.span(theme.t(" · accepted", " · 已接受"), Tone::Dim),
        ]
    } else {
        vec![
            theme.span("⚠ ", Tone::Bad),
            theme.span(head, Tone::Bad),
            theme.span(" · ", Tone::Faint),
            theme.span("a", Tone::Accent),
            theme.span(
                theme.t(" accept before publishing", " 接受后才能发布"),
                Tone::Dim,
            ),
        ]
    }
}

fn render_graph(state: &Confirm, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let canvas = graph::render(&state.effective, &state.view(usize::from(area.width)));
    let view_h = usize::from(area.height.saturating_sub(2));
    let view_w = usize::from(area.width);
    let (mut ox, mut oy) = (0usize, 0usize);
    let sel = canvas.boxes.iter().find(|b| b.0 == state.selected);
    if let Some(b) = sel {
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
    oy = snap_top(&canvas.boxes, oy);
    let above = canvas.boxes.iter().filter(|b| b.2 < oy).count();
    let below = canvas
        .boxes
        .iter()
        .filter(|b| b.2 + b.4 > oy + view_h)
        .count();
    let cells = match sel {
        Some(b) => graph::reveal(
            &canvas,
            (b.1 + b.3 / 2, b.2 + b.4 / 2),
            theme.frame(state.frame),
        ),
        None => canvas.cells.clone(),
    };
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
        &cells,
        (ox, oy),
    );
    // Say what is off screen instead of cutting it silently.
    if above > 0 {
        let text = if theme.zh {
            format!(" ↑ 上方还有 {above} 个 ")
        } else {
            format!(" ↑ {above} more above ")
        };
        let tw = width(&text) as u16;
        if area.width > tw {
            put(
                buf,
                area.right() - tw,
                area.y,
                tw,
                vec![theme.span(text, Tone::Dim)],
            );
        }
    }
    let total = state.effective.graph.entities.len();
    let status = match (&canvas.focus, theme.zh) {
        (Some(f), false) => format!(
            "focus {} · {} of {total} entities · one hop",
            label(&state.effective, f),
            canvas.shown.len()
        ),
        (Some(f), true) => format!(
            "聚焦 {} · {total} 个实体中的 {} 个 · 一跳",
            label(&state.effective, f),
            canvas.shown.len()
        ),
        (None, false) => format!(
            "{total} entities · {} relations",
            state.effective.graph.edges.len()
        ),
        (None, true) => format!(
            "{total} 个实体 · {} 条关系",
            state.effective.graph.edges.len()
        ),
    };
    let more = match (below > 0, theme.zh) {
        (false, _) => String::new(),
        (true, false) => format!(" · ↓ {below} more below"),
        (true, true) => format!(" · ↓ 下方还有 {below} 个"),
    };
    let mut spans = vec![
        theme.span("N─1 ", Tone::Accent),
        theme.span(theme.t("many-to-one  ", "多对一  "), Tone::Dim),
        theme.span("┄ ", Tone::Bad),
        theme.span(theme.t("rejected  ", "已拒绝  "), Tone::Dim),
    ];
    if area.width >= 90 {
        spans.push(theme.span("━ ", Tone::Accent));
        spans.push(theme.span(theme.t("selected  ", "选中  "), Tone::Dim));
    }
    spans.push(theme.span(format!(" {status}{more}"), Tone::Faint));
    put(buf, area.x, area.bottom() - 1, area.width, spans);
}

/// Move a vertical scroll offset up so no box is cut through its top edge.
fn snap_top(boxes: &[(String, usize, usize, usize, usize)], oy: usize) -> usize {
    boxes
        .iter()
        .filter(|b| b.2 < oy && oy < b.2 + b.4)
        .map(|b| b.2)
        .min()
        .unwrap_or(oy)
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
        let via = theme.t("via", "经");
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
                format!("  {} ", theme::pad(&format!("{via} {}", r.via), via_w - 1)),
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
        (theme.t("key", "主键"), e.key.join(", ")),
        (
            theme.t("display", "显示名"),
            e.display.clone().unwrap_or_else(|| "—".into()),
        ),
        (
            theme.t("rows", "行数"),
            e.rows.map_or(theme.t("unknown", "未知").into(), |r| {
                format!("{} ({r})", magnitude(r))
            }),
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
        vec![theme.span(theme.t("relations", "关系"), Tone::Dim)],
    );
    y += 1;
    let via = theme.t("via", "经");
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
                theme.span(format!("  {via} {}", line.via), Tone::Dim),
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
            vec![theme.span(
                theme.t("decisions on this table", "这张表上的决策"),
                Tone::Dim,
            )],
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
                    theme.span(theme::pad(class_word(&item.class, theme), 11), Tone::Dim),
                    theme.span(theme::pad(&subject(adj, item), 22), Tone::Ink),
                    theme.span(
                        fit(
                            &reason_text(item, theme),
                            usize::from(area.width).saturating_sub(40),
                        ),
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
        theme.t("decisions", "决策"),
        theme.t("by data evidence", "依据数据证据"),
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
        let conf = item.scores.get("confidence").and_then(Value::as_f64);
        let w = usize::from(area.width);
        let subj_w = w.saturating_sub(2 + 2 + 10 + 11 + 11);
        let subject_text = format!(
            "{} ",
            theme::pad(&subject(&state.effective, item), subj_w.saturating_sub(1))
        );
        let mut spans = vec![
            theme.span(if on { "▸ " } else { "  " }, Tone::Accent),
            theme.span(format!("{g} "), t),
            theme.span(theme::pad(class_word(&item.class, theme), 10), Tone::Dim),
            theme.span(subject_text.clone(), Tone::Ink),
            theme.span(
                theme::pad(
                    &format!(
                        "{}{}",
                        if changed { "✎" } else { "" },
                        decision_word(&item.decision, theme)
                    ),
                    11,
                ),
                if changed { Tone::Accent } else { t },
            ),
        ];
        // Confidence as pips plus the number (after ascii.rest cpu-meters).
        if let Some(c) = conf {
            let (on_pips, off_pips) = fx::pips(c);
            spans.push(theme.span(on_pips, if c >= 0.8 { Tone::Ember } else { Tone::Warn }));
            spans.push(theme.span(off_pips, Tone::Faint));
            spans.push(theme.span(format!("{:>5}", number(&serde_json::json!(c))), Tone::Dim));
        }
        if on && (panel || state.tab == Tab::Decisions) {
            spans[3] = Span::styled(subject_text, theme.selected());
        }
        put(buf, area.x, y, area.width, spans);
        // Evidence: containment as a bar with its threshold tick, then the
        // rest of the evidence (or what was missing) in words.
        let mut ev = Vec::new();
        let mut used = 0usize;
        let containment = item.evidence.get("containment").and_then(Value::as_f64);
        if let Some(c) = containment {
            let need = adjudication::needed_coverage(item, &state.effective.thresholds);
            // Passing needs both row and distinct coverage over the threshold.
            let low = item
                .evidence
                .get("distinct_containment")
                .and_then(Value::as_f64)
                .map_or(c, |d| d.min(c));
            let cols = 10;
            ev.push(theme.span(theme.t("contain ", "包含 "), Tone::Faint));
            ev.push(theme.span("▕", Tone::Faint));
            for (ch, filled) in fx::evidence_bar(c, need, cols) {
                let tone = match (filled, low >= need) {
                    (true, true) => Tone::Ok,
                    (true, false) => Tone::Bad,
                    (false, _) if ch == '┊' => Tone::Ink,
                    (false, _) => Tone::Faint,
                };
                ev.push(theme.span(ch.to_string(), tone));
            }
            ev.push(theme.span("▏", Tone::Faint));
            let cmp = if low >= need { "≥" } else { "<" };
            let txt = format!(
                " {}{cmp}{}  ",
                number(&serde_json::json!(c)),
                number(&serde_json::json!(need))
            );
            used += width(theme.t("contain ", "包含 ")) + cols + 2 + width(&txt);
            ev.push(theme.span(txt, if low >= need { Tone::Dim } else { Tone::Bad }));
        }
        let words = if item.decision == "rejected" && containment.is_none() {
            adjudication::missing_evidence(item, &state.effective.thresholds)
        } else if containment.is_some() {
            evidence_rest(item)
        } else {
            evidence_line(item)
        };
        ev.push(theme.span(
            fit(
                &localise_evidence(&words, theme, item.class == "relation"),
                w.saturating_sub(4 + used),
            ),
            Tone::Faint,
        ));
        put(buf, area.x + 4, y + 1, area.width.saturating_sub(4), ev);
        if per == 3 {
            put(
                buf,
                area.x + 4,
                y + 2,
                area.width.saturating_sub(4),
                vec![theme.span(
                    fit(&reason_text(item, theme), w.saturating_sub(4)),
                    Tone::Faint,
                )],
            );
        }
    }
}

/// Evidence words without the containment figure (drawn as a bar).
fn evidence_rest(item: &Item) -> String {
    let mut rest = item.clone();
    rest.evidence.remove("containment");
    evidence_line(&rest)
}

/// Evidence words in the interface language: each `·`-separated part keeps
/// its figures, its English name is swapped for the Chinese one.
fn localise_evidence(words: &str, theme: &Theme, relation: bool) -> String {
    if !theme.zh {
        return words.to_owned();
    }
    // Relation coverage reads as coverage, not as a column's distinct ratio.
    const RELATION: &[(&str, &str)] = &[("distinct", "去重覆盖"), ("rows", "行覆盖")];
    // Longer names first: a part matches by `name ` prefix or ` name` suffix.
    const NAMES: &[(&str, &str)] = &[
        ("parent not unique", "父键不唯一"),
        ("parent unique", "父键唯一"),
        ("many-to-one", "多对一"),
        ("one-to-many", "一对多"),
        ("one-to-one", "一对一"),
        ("many-to-many", "多对多"),
        ("format hits", "格式命中"),
        ("name hit", "名称命中"),
        ("name mapping", "名称映射"),
        ("recompute match", "重算一致"),
        ("unit consistent", "单位一致"),
        ("window stable", "窗口稳定"),
        ("non empty", "非空"),
        ("non-null", "非空率"),
        ("declared fk", "声明外键"),
        ("declared pk", "声明主键"),
        ("declared unique", "声明唯一"),
        ("observed unique", "实测唯一"),
        ("inferred id", "推断 ID"),
        ("discovered", "开放发现"),
        ("scoped", "需限定范围"),
        ("scope", "限定"),
        ("fan-out", "扇出"),
        ("unmatched", "未匹配"),
        ("collisions", "处冲突"),
        ("as", "角色"),
        ("distinct", "区分度"),
        ("containment", "包含度"),
        ("joins", "JOIN 共现"),
        ("name", "命名"),
        ("rows", "行"),
        ("parent", "父键"),
        ("not adopted", "未采纳"),
    ];
    let first: &[(&str, &str)] = if relation { RELATION } else { &[] };
    words
        .split(" · ")
        .map(|part| {
            for (en, zh) in first.iter().chain(NAMES) {
                if part == *en {
                    return (*zh).to_owned();
                }
                if let Some(rest) = part.strip_prefix(&format!("{en} ")) {
                    return format!("{zh} {rest}");
                }
                if let Some(head) = part.strip_suffix(&format!(" {en}")) {
                    return format!("{head} {zh}");
                }
            }
            part.to_owned()
        })
        .collect::<Vec<_>>()
        .join(" · ")
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
        snapshot::check(
            "4-confirm-decisions",
            80,
            24,
            Theme::MONO.zh(),
            |area, buf, theme| render(&dec, area, buf, theme),
        );
        let mut open = Confirm::new(adjudication::tests::incomplete(), "2026-11-09T00:00:00Z");
        open.tab = Tab::Decisions;
        open.width = 80;
        for theme in [Theme::MONO, Theme::MONO.zh()] {
            snapshot::check("4-confirm-incomplete", 80, 24, theme, |area, buf, theme| {
                render(&open, area, buf, theme)
            });
        }
        let mut accept = open.clone();
        accept.modal = Some(Modal::AcceptIncomplete {
            reason: "budget spent; resume next week".into(),
            error: None,
        });
        for theme in [Theme::MONO, Theme::MONO.zh()] {
            snapshot::check(
                "4-confirm-accept-incomplete",
                80,
                24,
                theme,
                |area, buf, theme| render(&accept, area, buf, theme),
            );
        }
    }

    #[test]
    fn incomplete_discovery_must_be_accepted_before_confirming() {
        let mut s = Confirm::new(adjudication::tests::incomplete(), "2026-11-09T00:00:00Z");
        s.width = 80;
        assert!(s.needs_acceptance());
        assert!(hints(&s).iter().any(|h| h.0 == "a"));
        let text = |s: &Confirm, theme: &Theme| {
            let mut buf = Buffer::empty(Rect::new(0, 0, 80, 24));
            render(s, buf.area, &mut buf, theme);
            super::super::buffer_text(&buf)
        };
        let shown = text(&s, &Theme::MONO);
        assert!(
            shown.contains("⚠ incomplete: 2 relations not checked · a accept before publishing"),
            "{shown}"
        );
        assert!(text(&s, &Theme::MONO.zh()).contains("未查完：2 条关系未检查"));
        // Enter goes to the acceptance step, not the confirmation.
        press(&mut s, &[KeyCode::Enter]);
        assert!(matches!(s.modal, Some(Modal::AcceptIncomplete { .. })));
        press(&mut s, &[KeyCode::Enter]);
        assert!(matches!(
            s.modal,
            Some(Modal::AcceptIncomplete { error: Some(_), .. })
        ));
        assert!(s.needs_acceptance(), "empty reason refused");
        typed(&mut s, "budget spent; resume next week");
        press(&mut s, &[KeyCode::Enter]);
        assert!(s.modal.is_none());
        assert!(!s.needs_acceptance());
        assert_eq!(
            s.overrides.items[DISCOVERY_OVERRIDE].decision,
            ACCEPT_INCOMPLETE
        );
        assert_eq!(s.effective.summary_sha256, s.base.summary_sha256);
        assert!(
            text(&s, &Theme::MONO).contains("✓ incomplete: 2 relations not checked · accepted")
        );
        // No item override was made, so the tab bar shows none.
        assert!(!text(&s, &Theme::MONO).contains("override(s)"));
        press(&mut s, &[KeyCode::Enter]);
        assert!(matches!(s.modal, Some(Modal::Confirm { .. })));
        typed(&mut s, "first publish");
        assert_eq!(press(&mut s, &[KeyCode::Enter]), Action::Next);
        let doc = s.confirmed.unwrap();
        assert_eq!(
            doc["summary_sha256"],
            serde_json::json!(s.base.summary_sha256)
        );
        assert_eq!(
            doc["overrides_sha256"],
            serde_json::json!(adjudication::overrides_sha256(&s.overrides))
        );
        // A complete discovery shows no banner and needs no acceptance.
        let done = Confirm::new(fixture(), "t");
        assert!(!done.needs_acceptance());
        assert!(!text(&done, &Theme::MONO).contains("incomplete:"));
    }

    #[test]
    fn fan_out_relation_cannot_be_flipped_and_says_why() {
        let mut s = Confirm::new(fixture(), "t");
        s.tab = Tab::Relations;
        s.relation = s
            .effective
            .graph
            .edges
            .iter()
            .position(|e| e.relation == "relation/orders.route_code")
            .unwrap();
        press(&mut s, &[KeyCode::Char('o')]);
        let Some(Modal::Override {
            error: Some(why), ..
        }) = &s.modal
        else {
            panic!("fan-out flip must show why: {:?}", s.modal);
        };
        assert_eq!(
            why,
            "parent key repeats (fan-out ×3); filter on valid_from first"
        );
        assert_eq!(
            error_text(why, &Theme::MONO.zh()),
            "父键重复（扇出 ×3）；先按 valid_from 过滤"
        );
        typed(&mut s, "trust me");
        press(&mut s, &[KeyCode::Enter]);
        assert!(s.overrides.items.is_empty(), "flip refused");
        assert!(matches!(
            s.modal,
            Some(Modal::Override { error: Some(_), .. })
        ));
    }

    #[test]
    fn decisions_localise_relation_coverage() {
        let words = "rows 0.71 · distinct 0.75 · 284k unmatched · fan-out ×3 · scope valid_from · as dest_station · 2 collisions";
        assert_eq!(
            localise_evidence(words, &Theme::MONO.zh(), true),
            "行覆盖 0.71 · 去重覆盖 0.75 · 284k 未匹配 · 扇出 ×3 · 限定 valid_from · 角色 dest_station · 2 处冲突"
        );
        assert_eq!(
            localise_evidence("distinct 0.97 · format hits 1", &Theme::MONO.zh(), false),
            "区分度 0.97 · 格式命中 1"
        );
        let item = fixture()
            .items
            .into_iter()
            .find(|i| i.rule_id == "relation.scope_required")
            .unwrap();
        assert_eq!(
            reason_text(&item, &Theme::MONO.zh()),
            "父键重复，按 valid_from 过滤后才唯一；请先过滤父表"
        );
        let mut quiet = item.clone();
        quiet.rule_id = "relation.no_signal".into();
        quiet.reason = "Values fit, but no name, declaration or query links these columns.".into();
        assert_eq!(reason_text(&quiet, &Theme::MONO), quiet.reason);
        assert!(reason_text(&quiet, &Theme::MONO.zh()).starts_with("值能对上"));
        assert_eq!(
            localise_evidence("inferred id · discovered", &Theme::MONO.zh(), true),
            "推断 ID · 开放发现"
        );
    }

    #[test]
    fn graph_view_never_cuts_a_box_top() {
        for (w, h) in [(80u16, 24u16), (120, 40)] {
            let s = Confirm::new(fixture(), "t");
            let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
            render(&s, buf.area, &mut buf, &Theme::MONO);
            let text = super::super::buffer_text(&buf);
            // The first graph row (inside frame, tab bar and blank) must not
            // start mid-box: no box side without a top edge above it.
            let row = text.lines().nth(3).unwrap();
            let first = row.trim_start_matches('│').trim();
            assert!(
                !first.starts_with('│') && !first.starts_with('┃'),
                "{w}x{h} first graph row cuts a box: {row}"
            );
        }
    }

    #[test]
    fn reveal_frames() {
        let s = Confirm::new(fixture(), "t");
        let frames: Vec<(usize, &str)> = (0..REVEAL_FRAMES)
            .step_by(2)
            .map(|f| (f, "radar reveal from the selected hub"))
            .chain([(usize::MAX, "still")])
            .collect();
        snapshot::frames(
            "4-confirm-graph",
            80,
            24,
            false,
            &frames,
            |f, area, buf, theme| {
                let mut c = s.clone();
                c.frame = f;
                render(&c, area, buf, theme);
            },
        );
    }
}
