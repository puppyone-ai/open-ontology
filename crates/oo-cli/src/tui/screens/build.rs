//! Screen 3: build. A five-stage timeline (load → profile → draft →
//! adjudicate → compile) with smooth bars, counts and ETA. The state is read
//! from the build chain's saved state, so it renders the same whether the
//! build runs in this process, a container, or over SSH.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use serde_json::Value;

use super::{Status, put};
use crate::tui::fx;
use crate::tui::theme::{self, Theme, Tone, heading};

pub const STAGES: [(&str, &str); 5] = [
    ("load", "copy selected tables into the private replica"),
    ("profile", "measure keys, containment, formats, cardinality"),
    ("draft", "draft the ontology (model, bounded calls)"),
    ("adjudicate", "decide warn items from data evidence"),
    ("compile", "compile and verify the role pack"),
];

#[derive(Clone, Debug, PartialEq)]
pub struct Stage {
    pub name: String,
    pub status: Status,
    pub completed: u64,
    pub total: u64,
    pub unit: String,
    pub elapsed_s: Option<u64>,
    pub eta_s: Option<u64>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Build {
    pub stages: Vec<Stage>,
    pub frame: usize,
    pub note: String,
}

impl Build {
    pub fn new() -> Self {
        Build {
            stages: STAGES
                .iter()
                .map(|(name, _)| Stage {
                    name: (*name).into(),
                    status: Status::Waiting,
                    completed: 0,
                    total: 0,
                    unit: String::new(),
                    elapsed_s: None,
                    eta_s: None,
                    error: None,
                })
                .collect(),
            frame: 0,
            note: String::new(),
        }
    }

    pub fn done(&self) -> bool {
        self.stages.iter().all(|s| s.status == Status::Pass)
    }

    pub fn failed(&self) -> bool {
        self.stages.iter().any(|s| s.status == Status::Fail)
    }
}

impl Default for Build {
    fn default() -> Self {
        Build::new()
    }
}

/// Map a chain stage name onto one of the five product stages.
pub fn product_stage(name: &str) -> Option<usize> {
    let n = name.to_ascii_lowercase();
    Some(
        if n.contains("load") || n.contains("snapshot") || n.contains("copy") {
            0
        } else if n.contains("profil") || n.contains("catalog") {
            1
        } else if n.contains("draft") || n.contains("generation") || n.contains("selfbuild") {
            2
        } else if n.contains("adjudic") || n.contains("review") {
            3
        } else if n.contains("compil") || n.contains("publi") || n.contains("pack") {
            4
        } else {
            return None;
        },
    )
}

/// Fold saved chain state (`{"stages":[{name,status,completed,total,...}]}`)
/// into the five-stage view.
pub fn from_chain(state: &Value) -> Build {
    let mut build = Build::new();
    for stage in state["stages"].as_array().into_iter().flatten() {
        let Some(index) = stage["name"].as_str().and_then(product_stage) else {
            continue;
        };
        let target = &mut build.stages[index];
        let status = match stage["status"].as_str().unwrap_or("") {
            "done" | "completed" | "ok" | "passed" | "published" => Status::Pass,
            "running" | "in_progress" | "started" => Status::Running,
            "failed" | "blocked" | "error" => Status::Fail,
            _ => Status::Waiting,
        };
        // A product stage made of several chain stages is only as far as its
        // least advanced part.
        target.status = match (target.status, status) {
            (Status::Fail, _) | (_, Status::Fail) => Status::Fail,
            (Status::Waiting, s) => s,
            (Status::Pass, Status::Pass) => Status::Pass,
            _ => Status::Running,
        };
        target.completed += stage["completed"].as_u64().unwrap_or(0);
        target.total += stage["total"].as_u64().unwrap_or(0);
        if let Some(unit) = stage["unit"].as_str() {
            target.unit = unit.into();
        }
        target.elapsed_s = stage["elapsed_seconds"].as_u64().or(target.elapsed_s);
        target.eta_s = stage["eta_seconds"].as_u64().or(target.eta_s);
        if let Some(error) = stage["error"].as_str() {
            target.error = Some(error.into());
        }
    }
    build
}

fn clock(secs: u64) -> String {
    if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

pub const HINTS: [(&str, &str); 2] = [
    ("q", "stop watching (the build continues)"),
    ("⏎", "continue when done"),
];

pub fn render(state: &Build, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let inner = theme::frame(buf, area, theme, 2, &HINTS);
    if inner.height < 8 {
        return;
    }
    let done = state
        .stages
        .iter()
        .filter(|s| s.status == Status::Pass)
        .count();
    let eta: u64 = state.stages.iter().filter_map(|s| s.eta_s).sum();
    let note = if state.failed() {
        "stopped".to_owned()
    } else if state.done() {
        "done".to_owned()
    } else if eta > 0 {
        format!("about {} left", clock(eta))
    } else {
        "estimating".to_owned()
    };
    heading(
        buf,
        inner.x,
        inner.y,
        inner.width,
        theme,
        "build",
        &format!("{done}/5 · {note}"),
    );
    let wide = inner.width >= 100;
    let bar_w = if wide { 32 } else { 20 };
    let spacing = if inner.height >= 30 {
        3
    } else if inner.height >= 18 {
        2
    } else {
        1
    };
    let mut y = inner.y + 2;
    for (index, stage) in state.stages.iter().enumerate() {
        if y >= inner.bottom() {
            break;
        }
        let (node, tone) = match stage.status {
            Status::Pass => ("●", Tone::Ok),
            Status::Running => ("◉", Tone::Accent),
            Status::Fail => ("✗", Tone::Bad),
            _ => ("○", Tone::Faint),
        };
        let fraction = match stage.status {
            Status::Pass => 1.0,
            _ if stage.total > 0 => stage.completed as f64 / stage.total as f64,
            _ => 0.0,
        };
        let (fill, rest) = fx::bar(fraction, bar_w);
        let count = if stage.total > 0 {
            format!("{}/{} {}", stage.completed, stage.total, stage.unit)
        } else {
            String::new()
        };
        let time = match (stage.status, stage.elapsed_s, stage.eta_s) {
            (Status::Pass, Some(e), _) => clock(e),
            (Status::Running, _, Some(eta)) => format!("eta {}", clock(eta)),
            _ => String::new(),
        };
        let mut spans = vec![
            theme.span(format!("{node} "), tone),
            theme.span(
                theme::pad(&stage.name, 11),
                if stage.status == Status::Waiting {
                    Tone::Dim
                } else {
                    Tone::Ink
                },
            ),
            theme.span(
                fill,
                if stage.status == Status::Fail {
                    Tone::Bad
                } else {
                    tone
                },
            ),
            theme.span(rest, Tone::Faint),
            theme.span(format!("  {}", theme::pad(&count, 18)), Tone::Dim),
            theme.span(theme::pad(&time, 9), Tone::Dim),
        ];
        if stage.status == Status::Running {
            spans.push(theme.span(fx::spinner(state.frame).to_string(), Tone::Accent));
        }
        put(buf, inner.x, y, inner.width, spans);
        let detail = match &stage.error {
            Some(error) => (format!("✗ {error}"), Tone::Bad),
            None => (STAGES[index].1.to_owned(), Tone::Faint),
        };
        if spacing > 1 && y + 1 < inner.bottom() {
            let rail = if index + 1 < state.stages.len() {
                if stage.status == Status::Pass {
                    "│ "
                } else {
                    "┆ "
                }
            } else {
                "  "
            };
            put(
                buf,
                inner.x,
                y + 1,
                inner.width,
                vec![
                    theme.span(rail, Tone::Faint),
                    theme.span(format!("           {}", detail.0), detail.1),
                ],
            );
            for k in 2..spacing {
                if index + 1 < state.stages.len() && y + k < inner.bottom() {
                    put(buf, inner.x, y + k, 2, vec![theme.span(rail, Tone::Faint)]);
                }
            }
        }
        y += spacing;
    }
    if !state.note.is_empty() && y + 1 < inner.bottom() {
        put(
            buf,
            inner.x,
            inner.bottom() - 1,
            inner.width,
            vec![theme.span(state.note.clone(), Tone::Dim)],
        );
    }
    super::ambient(buf, inner, theme);
}

#[cfg(test)]
mod tests {
    use super::super::snapshot;
    use super::*;
    use serde_json::json;

    pub(crate) fn sample() -> Build {
        let mut b = from_chain(&json!({"stages":[
            {"name":"load","status":"done","completed":12,"total":12,"unit":"tables","elapsed_seconds":42},
            {"name":"profile","status":"running","completed":7,"total":12,"unit":"tables","eta_seconds":31},
            {"name":"generation","status":"pending"},
        ]}));
        b.frame = 2;
        b
    }

    #[test]
    fn folds_chain_state() {
        let b = sample();
        assert_eq!(b.stages[0].status, Status::Pass);
        assert_eq!(b.stages[1].status, Status::Running);
        assert_eq!(b.stages[2].status, Status::Waiting);
        let failed = from_chain(
            &json!({"stages":[{"name":"selfbuild","status":"failed","error":"MODEL_BUDGET"}]}),
        );
        assert!(failed.failed());
        assert_eq!(product_stage("publication"), Some(4));
    }

    #[test]
    fn snapshots() {
        let state = sample();
        snapshot::all_sizes("3-build", |area, buf, theme| {
            render(&state, area, buf, theme)
        });
    }
}
