//! Screen 3: build. A five-stage timeline (load → profile → draft →
//! adjudicate → compile) with smooth bars, counts and ETA. The state is read
//! from the build chain's saved state, so it renders the same whether the
//! build runs in this process, a container, or over SSH.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use serde_json::Value;

use super::{Status, put};
use crate::tui::fx;
use crate::tui::theme::{self, Hint, Theme, Tone, heading};

/// `(name, zh name, what it does, zh)`.
pub const STAGES: [(&str, &str, &str, &str); 5] = [
    (
        "load",
        "载入",
        "copy selected tables into the private replica",
        "把选中的表复制到私有副本",
    ),
    (
        "profile",
        "画像",
        "measure keys, containment, formats, cardinality",
        "测量键、包含度、格式和基数",
    ),
    (
        "draft",
        "起草",
        "draft the ontology (model, bounded calls)",
        "起草本体（模型，有调用上限）",
    ),
    (
        "adjudicate",
        "裁决",
        "decide warn items from data evidence",
        "按数据证据裁决待定项",
    ),
    (
        "compile",
        "编译",
        "compile and verify the role pack",
        "编译并校验角色包",
    ),
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
    /// Units completed across all stages, sampled once a second by the
    /// runner (oldest first); drives the throughput sparkline.
    pub history: Vec<u64>,
}

impl Build {
    pub fn new() -> Self {
        Build {
            stages: STAGES
                .iter()
                .map(|(name, ..)| Stage {
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
            history: Vec::new(),
        }
    }

    /// Units completed so far, all stages together.
    pub fn completed(&self) -> u64 {
        self.stages.iter().map(|s| s.completed).sum()
    }

    /// Overall progress, each stage weighing one fifth.
    pub fn fraction(&self) -> f64 {
        self.stages.iter().map(stage_fraction).sum::<f64>() / self.stages.len().max(1) as f64
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
    let mut seen = [false; 5];
    for stage in state["stages"].as_array().into_iter().flatten() {
        let Some(index) = stage["name"].as_str().and_then(product_stage) else {
            continue;
        };
        let target = &mut build.stages[index];
        let status = match stage["status"].as_str().unwrap_or("") {
            "done" | "complete" | "completed" | "ok" | "passed" | "published" => Status::Pass,
            "running" | "in_progress" | "started" => Status::Running,
            "failed" | "blocked" | "error" => Status::Fail,
            _ => Status::Waiting,
        };
        // A product stage made of several chain stages is only as far as its
        // least advanced part.
        target.status = if !seen[index] {
            status
        } else {
            match (target.status, status) {
                (Status::Fail, _) | (_, Status::Fail) => Status::Fail,
                (Status::Waiting, Status::Waiting) => Status::Waiting,
                (Status::Pass, Status::Pass) => Status::Pass,
                _ => Status::Running,
            }
        };
        seen[index] = true;
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

pub const HINTS: [Hint; 2] = [
    (
        "q",
        "stop watching (the build continues)",
        "停止观看（构建继续）",
    ),
    ("⏎", "continue when done", "完成后继续"),
];

fn stage_fraction(stage: &Stage) -> f64 {
    match stage.status {
        Status::Pass => 1.0,
        _ if stage.total > 0 => (stage.completed as f64 / stage.total as f64).min(1.0),
        _ => 0.0,
    }
}

/// The overall gauge: one segment a stage, a blank between segments; done
/// stages in the deep warm tone, the running one in the accent with a heat
/// edge (`▓▒░`) where it is working, failed in red.
fn gauge(state: &Build, cols: usize, theme: &Theme) -> Vec<ratatui::text::Span<'static>> {
    let n = state.stages.len();
    let seg = (cols.saturating_sub(n - 1) / n).max(2);
    let mut spans = Vec::new();
    for (k, stage) in state.stages.iter().enumerate() {
        if k > 0 {
            spans.push(theme.span(" ", Tone::Faint));
        }
        let f = stage_fraction(stage);
        let filled = (f * seg as f64).round() as usize;
        let tone = match stage.status {
            Status::Pass => Tone::Ember,
            Status::Fail => Tone::Bad,
            _ => Tone::Accent,
        };
        let mut done = "█".repeat(filled.min(seg));
        let mut rest = String::new();
        let mut edge = String::new();
        if stage.status == Status::Running && filled < seg {
            for (i, c) in ['▓', '▒', '░'].iter().enumerate() {
                if filled + i < seg {
                    edge.push(*c);
                }
            }
        }
        let used = filled.min(seg) + edge.chars().count();
        rest.push_str(&"·".repeat(seg.saturating_sub(used)));
        if stage.status == Status::Fail && filled == 0 {
            done = "✗".to_owned();
            rest = "·".repeat(seg - 1);
        }
        spans.push(theme.span(done, tone));
        spans.push(theme.span(edge, Tone::Ember));
        spans.push(theme.span(rest, Tone::Faint));
    }
    spans
}

/// One stage's bar. Few units: whole units, like an uptime bar (`█` done,
/// `▒` the one in hand, `·` waiting). Many: a smooth eighth-block bar.
/// A running bar carries a skeleton shimmer over its filled part.
fn stage_bar(
    stage: &Stage,
    cols: usize,
    frame: usize,
    theme: &Theme,
) -> Vec<ratatui::text::Span<'static>> {
    let tone = match stage.status {
        Status::Pass => Tone::Ok,
        Status::Fail => Tone::Bad,
        Status::Running => Tone::Accent,
        _ => Tone::Faint,
    };
    let mut cells: Vec<(char, Tone)> = if stage.total > 0 && stage.total as usize <= cols {
        let total = stage.total as usize;
        let done = if stage.status == Status::Pass {
            total
        } else {
            stage.completed as usize
        };
        (0..cols)
            .map(|c| {
                let unit = c * total / cols;
                if unit < done {
                    ('█', tone)
                } else if unit == done && stage.status == Status::Running {
                    ('▒', Tone::Accent)
                } else {
                    ('·', Tone::Faint)
                }
            })
            .collect()
    } else {
        let (done, rest) = fx::bar(stage_fraction(stage), cols);
        done.chars()
            .map(|c| (c, tone))
            .chain(rest.chars().map(|c| (c, Tone::Faint)))
            .collect()
    };
    if stage.status == Status::Running {
        let filled = cells.iter().take_while(|c| c.0 == '█').count();
        if let Some(at) = fx::shimmer(filled, frame) {
            let end = (at + 1).min(filled.saturating_sub(1));
            for (x, cell) in cells
                .iter_mut()
                .enumerate()
                .take(end + 1)
                .skip(at.saturating_sub(1))
            {
                if cell.0 == '█' {
                    *cell = (if x == at { '▓' } else { '█' }, Tone::Hot);
                }
            }
        }
    }
    let mut spans = vec![theme.span("▕", Tone::Faint)];
    for (c, t) in cells {
        spans.push(theme.span(c.to_string(), t));
    }
    spans.push(theme.span("▏", Tone::Faint));
    spans
}

pub fn render(state: &Build, area: Rect, buf: &mut Buffer, theme: &Theme) {
    let inner = theme::frame(buf, area, theme, 2, &HINTS);
    if inner.height < 8 {
        return;
    }
    let frame = theme.frame(state.frame);
    let done = state
        .stages
        .iter()
        .filter(|s| s.status == Status::Pass)
        .count();
    let eta: u64 = state.stages.iter().filter_map(|s| s.eta_s).sum();
    let note = if state.failed() {
        theme.t("stopped", "已停止").to_owned()
    } else if state.done() {
        theme.t("done", "完成").to_owned()
    } else if eta > 0 {
        if theme.zh {
            format!("约剩 {}", clock(eta))
        } else {
            format!("about {} left", clock(eta))
        }
    } else {
        theme.t("estimating", "估算中").to_owned()
    };
    heading(
        buf,
        inner.x,
        inner.y,
        inner.width,
        theme,
        theme.t("build", "构建"),
        &format!("{done}/5 · {note}"),
    );
    // Overall gauge and its percentage.
    let pct = (state.fraction() * 100.0).floor() as u64;
    let pct_text = format!("{pct:>3}%");
    let gauge_w = usize::from(inner.width).saturating_sub(pct_text.len() + 2);
    let mut spans = gauge(state, gauge_w, theme);
    spans.push(theme.span("  ", Tone::Faint));
    spans.push(theme.span(pct_text, if state.done() { Tone::Ok } else { Tone::Accent }));
    put(buf, inner.x, inner.y + 2, inner.width, spans);

    let wide = inner.width >= 100;
    let bar_w = if wide { 36 } else { 20 };
    let spacing = if inner.height >= 30 { 3 } else { 2 };
    let mut y = inner.y + 4;
    let rail_x = inner.x;
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
        let (name_en, name_zh, desc_en, desc_zh) = STAGES[index];
        let count = if stage.total > 0 {
            format!("{}/{} {}", stage.completed, stage.total, stage.unit)
        } else {
            String::new()
        };
        let time = match (stage.status, stage.elapsed_s, stage.eta_s) {
            (Status::Pass, Some(e), _) => clock(e),
            (Status::Running, _, Some(eta)) => {
                format!("{} {}", theme.t("eta", "预计"), clock(eta))
            }
            _ => String::new(),
        };
        let word = match stage.status {
            Status::Pass => theme.t("passed", "完成"),
            Status::Running | Status::Warn => theme.t("running", "进行中"),
            Status::Fail => theme.t("failed", "失败"),
            Status::Waiting => theme.t("queued", "排队"),
        };
        let mut spans = vec![
            theme.span(format!("{node} "), tone),
            theme.span(
                theme::pad(theme.t(name_en, name_zh), 11),
                if stage.status == Status::Waiting {
                    Tone::Dim
                } else {
                    Tone::Ink
                },
            ),
        ];
        spans.extend(stage_bar(stage, bar_w, frame, theme));
        spans.push(theme.span(format!(" {}", theme::pad(&count, 15)), Tone::Dim));
        spans.push(theme.span(theme::pad(&time, 9), Tone::Dim));
        spans.push(theme.span(
            word,
            match stage.status {
                Status::Pass => Tone::Ok,
                Status::Fail => Tone::Bad,
                Status::Waiting => Tone::Faint,
                _ => Tone::Accent,
            },
        ));
        if stage.status == Status::Running && frame != usize::MAX {
            spans.push(theme.span(format!(" {}", fx::spinner(frame)), Tone::Accent));
        }
        put(buf, inner.x, y, inner.width, spans);
        let detail = match &stage.error {
            Some(error) => (format!("✗ {error}"), Tone::Bad),
            None => (
                theme.t(desc_en, desc_zh).to_owned(),
                if stage.status == Status::Running {
                    Tone::Dim
                } else {
                    Tone::Faint
                },
            ),
        };
        if y + 1 < inner.bottom() {
            let last = index + 1 == state.stages.len();
            let rail = match (last, stage.status) {
                (true, _) => "  ",
                (false, Status::Pass) => "│ ",
                _ => "┆ ",
            };
            put(
                buf,
                rail_x,
                y + 1,
                inner.width,
                vec![
                    theme.span(
                        rail,
                        if stage.status == Status::Pass {
                            Tone::Ember
                        } else {
                            Tone::Faint
                        },
                    ),
                    theme.span(format!("           {}", detail.0), detail.1),
                ],
            );
            for k in 2..spacing {
                if !last && y + k < inner.bottom() {
                    put(buf, rail_x, y + k, 2, vec![theme.span(rail, Tone::Faint)]);
                }
            }
        }
        y += spacing;
    }
    // Throughput sparkline from the sampled history.
    let foot = inner.bottom() - 1;
    if state.history.len() >= 3 && y + 1 < foot {
        let deltas: Vec<u64> = state
            .history
            .windows(2)
            .map(|w| w[1].saturating_sub(w[0]))
            .collect();
        let unit = state
            .stages
            .iter()
            .find(|s| s.status == Status::Running)
            .map_or("", |s| s.unit.as_str());
        let last = deltas.last().copied().unwrap_or(0);
        let label = theme.t("pace", "速度");
        let tail = if theme.zh {
            format!("  {last} {unit}/秒")
        } else {
            format!("  {last} {unit}/s")
        };
        let spark_w = usize::from(inner.width)
            .saturating_sub(13 + theme::width(&tail))
            .min(40)
            .min(deltas.len());
        put(
            buf,
            inner.x,
            y + 1,
            inner.width,
            vec![
                theme.span(theme::pad(label, 13), Tone::Dim),
                theme.span(fx::spark(&deltas, spark_w), Tone::Ember),
                theme.span(tail, Tone::Dim),
            ],
        );
    }
    if !state.note.is_empty() {
        put(
            buf,
            inner.x,
            foot,
            inner.width,
            vec![theme.span(state.note.clone(), Tone::Dim)],
        );
    }
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
        b.history = vec![
            0, 1, 3, 6, 10, 14, 17, 19, 20, 21, 23, 26, 30, 33, 35, 36, 37, 39, 42, 46, 49, 51,
        ];
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
    fn real_chain_completion_and_pending_substages_are_order_independent() {
        let complete = from_chain(&json!({"stages":[{"name":"load","status":"complete"}]}));
        assert_eq!(complete.stages[0].status, Status::Pass);
        for statuses in [["pending", "complete"], ["complete", "pending"]] {
            let chain = from_chain(&json!({"stages":[
                {"name":"compile","status":statuses[0]},
                {"name":"publish","status":statuses[1]}
            ]}));
            assert_eq!(chain.stages[4].status, Status::Running);
            assert!(!chain.done());
        }
    }

    #[test]
    fn snapshots() {
        let state = sample();
        snapshot::all_sizes("3-build", |area, buf, theme| {
            render(&state, area, buf, theme)
        });
        let failed = from_chain(&json!({"stages":[
            {"name":"load","status":"done","completed":12,"total":12,"unit":"tables","elapsed_seconds":42},
            {"name":"profile","status":"done","completed":12,"total":12,"unit":"tables","elapsed_seconds":61},
            {"name":"selfbuild","status":"failed","error":"MODEL_BUDGET: 40 calls used"},
        ]}));
        snapshot::check("3-build-failed", 80, 24, Theme::MONO, |area, buf, theme| {
            render(&failed, area, buf, theme)
        });
    }

    #[test]
    fn shimmer_frames() {
        let state = sample();
        let frames: Vec<(usize, &str)> = (0..8)
            .map(|f| (f, "shimmer runs over the profile bar"))
            .chain([(usize::MAX, "still")])
            .collect();
        snapshot::frames("3-build", 80, 24, false, &frames, |f, area, buf, theme| {
            let mut s = state.clone();
            s.frame = f;
            render(&s, area, buf, theme);
        });
    }
}
