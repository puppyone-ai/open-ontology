//! LongMemEval-V2 memory backend on the oo-docs engine (benchmark driver,
//! not shipped). Spoken to over stdio, one JSON object per line:
//!
//! * `{"op":"insert","trajectory":{..}}`: the trajectory becomes one
//!   markdown document `trajectories/<nnnn>-<id>.md` (a level 2 heading per
//!   state, the accessibility tree as a fenced block), parsed and normalized
//!   into `oo.doc-ir/v1` by the engine, run through the `rules/1`
//!   extraction, and its IR blocks are indexed in the `bm25/1` lexical
//!   channel. The haystack position is the valid time: when the same text
//!   was observed in several trajectories, the latest observation supersedes
//!   the earlier ones and is the one shown.
//! * `{"op":"query","text":..,"budget":N}`: ranks IR block windows and
//!   trajectory digests for the question and returns evidence items with
//!   provenance tags, at most about `N` tokens (`oo.token-estimate/v2`).
//!   Never an answer.
//!
//! Replies are `{"ok":true,..}` or `{"ok":false,"error":..}`.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, Write};

use oo_core::docir::{BlockKind, DocIr};
use oo_docs::compile::{source, tokens};
use oo_docs::extract::rules;
use oo_docs::parse::markdown::parse_markdown;
use oo_docs::vector::lexical::Index;
use serde_json::{Value, json};

/// Lines per indexed window of an accessibility tree.
const WINDOW: usize = 20;
/// Windows of context shown on each side of a hit window.
const SPREAD: usize = 1;

struct State {
    traj: usize,
    step: usize,
    url: String,
    title: String,
    tag: String,
    lines: Vec<String>,
    action: String,
    prev_action: String,
    thought: String,
}

struct Traj {
    ordinal: usize,
    goal: String,
    outcome: String,
    states: usize,
    path: String,
}

/// One indexed text, with every place it was observed: `(state, first line)`.
struct Unit {
    seen: Vec<(usize, usize)>,
}

#[derive(Default)]
struct Memory {
    trajs: Vec<Traj>,
    states: Vec<State>,
    windows: Index,
    units: Vec<Unit>,
    by_text: HashMap<blake3::Hash, u32>,
    digests: Index,
    digest_text: Vec<String>,
    blocks: usize,
    facts: usize,
}

fn s(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(x)) => x.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn clip(text: &str, max: usize) -> String {
    let t: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() <= max {
        t
    } else {
        format!("{}...", t.chars().take(max).collect::<String>())
    }
}

/// Page title from the first `RootWebArea '<title>'` line.
fn title_of(tree: &str) -> String {
    let first = tree.lines().next().unwrap_or("");
    match (first.find('\''), first.rfind('\'')) {
        (Some(a), Some(b)) if b > a => first[a + 1..b].to_string(),
        _ => clip(first, 80),
    }
}

/// `click('112')` -> `click('112') on [112] button 'Search'`.
fn resolve_action(action: &str, tree: &str) -> String {
    let action = action.trim();
    if action.is_empty() {
        return String::new();
    }
    let bid = action
        .split('\'')
        .nth(1)
        .filter(|b| !b.is_empty() && b.len() < 12 && !b.contains(' '));
    if let Some(bid) = bid {
        let needle = format!("[{bid}]");
        if let Some(line) = tree.lines().find(|l| l.contains(&needle)) {
            return format!("{} on {}", clip(action, 160), clip(line, 140));
        }
    }
    clip(action, 200)
}

fn fence_for(text: &str) -> String {
    let mut f = "````".to_string();
    while text.contains(&f) {
        f.push('`');
    }
    f
}

impl Memory {
    fn insert(&mut self, t: &Value) -> Result<Value, String> {
        let ordinal = self.trajs.len() + 1;
        let id = s(t, "id");
        let goal = s(t, "goal");
        let outcome = s(t, "outcome");
        let states = t
            .get("states")
            .and_then(Value::as_array)
            .ok_or("trajectory without states")?;
        let path = format!("trajectories/{ordinal:04}-{id}.md");

        // Render the markdown document the engine parses.
        let mut md = format!(
            "# {}\n\nTrajectory {id}, outcome {outcome}, {} states.\n\n",
            clip(&goal, 300),
            states.len()
        );
        let mut raw: Vec<(String, String, String, String)> = Vec::new();
        for (k, st) in states.iter().enumerate() {
            let tree = s(st, "accessibility_tree");
            let action = s(st, "action");
            let thought = s(st, "thought");
            let url = s(st, "url");
            md.push_str(&format!("## Step {k}: {}\n\n", clip(&title_of(&tree), 120)));
            md.push_str(&format!("URL: {}\n\n", url.trim()));
            let body = if tree.trim().is_empty() {
                "(empty)".to_string()
            } else {
                tree.clone()
            };
            let fence = fence_for(&body);
            md.push_str(&format!("{fence}text\n{}\n{fence}\n\n", body.trim_end()));
            raw.push((url, action, thought, tree));
        }
        let parsed = parse_markdown(md.as_bytes()).map_err(|e| format!("parse: {e}"))?;
        let ir: &DocIr = &parsed.ir;
        let extraction = rules::extract(&path, ir);
        self.blocks += ir.blocks.len();
        self.facts += extraction.facts.len();

        // IR blocks back to states: the `Step k` heading of `hpath` owns the
        // code blocks under it; the heading block's anchor is the state's tag.
        let base = self.states.len();
        let mut lines: Vec<Vec<String>> = vec![Vec::new(); raw.len()];
        let mut tags: Vec<String> = vec![String::new(); raw.len()];
        for b in &ir.blocks {
            let step = b
                .hpath
                .last()
                .or(if b.kind == BlockKind::Heading {
                    Some(&b.text)
                } else {
                    None
                })
                .and_then(|h| h.strip_prefix("Step "))
                .and_then(|h| h.split(':').next())
                .and_then(|n| n.trim().parse::<usize>().ok());
            let Some(k) = step.filter(|k| *k < raw.len()) else {
                continue;
            };
            if b.kind == BlockKind::Heading {
                tags[k] = source::tag(&path, &b.anchor);
            } else if b.kind == BlockKind::Code {
                lines[k].extend(b.text.lines().map(str::to_string));
            }
        }
        for (k, (url, action, thought, tree)) in raw.iter().enumerate() {
            let prev = if k > 0 {
                resolve_action(&raw[k - 1].1, &raw[k - 1].3)
            } else {
                String::new()
            };
            self.states.push(State {
                traj: ordinal - 1,
                step: k,
                url: url.trim().to_string(),
                title: title_of(tree),
                tag: std::mem::take(&mut tags[k]),
                lines: std::mem::take(&mut lines[k]),
                action: resolve_action(action, tree),
                prev_action: prev,
                thought: clip(thought, 400),
            });
        }

        // Index windows; a text seen before is the same unit (supersession by
        // haystack order happens at read time: the latest sighting wins).
        let mut digest = format!(
            "Trajectory {ordinal} ({id}). Goal: {}\nOutcome: {outcome}\nSteps:\n",
            clip(&goal, 600)
        );
        for si in base..self.states.len() {
            let head = {
                let st = &self.states[si];
                format!(
                    "{} {} {} {} {}",
                    st.title, st.url, st.prev_action, st.action, st.thought
                )
            };
            self.add_unit(&head, si, usize::MAX);
            let n = self.states[si].lines.len();
            let mut start = 0;
            while start < n {
                let end = (start + WINDOW).min(n);
                let text = self.states[si].lines[start..end].join("\n");
                let with_title = format!("{}\n{text}", self.states[si].title);
                self.add_unit(&with_title, si, start);
                start = end;
            }
            let st = &self.states[si];
            digest.push_str(&format!(
                "{}. [{}] {}{}\n",
                st.step,
                clip(&st.title, 80),
                if st.action.is_empty() {
                    "(end)".to_string()
                } else {
                    clip(&st.action, 200)
                },
                if st.thought.is_empty() {
                    String::new()
                } else {
                    format!(" | thought: {}", clip(&st.thought, 220))
                }
            ));
        }
        self.digests.add(&digest);
        self.digest_text.push(digest);
        self.trajs.push(Traj {
            ordinal,
            goal,
            outcome,
            states: raw.len(),
            path,
        });
        Ok(json!({"ok": true, "blocks": ir.blocks.len(), "facts": extraction.facts.len()}))
    }

    fn add_unit(&mut self, text: &str, state: usize, line: usize) {
        let h = blake3::hash(text.as_bytes());
        if let Some(&u) = self.by_text.get(&h) {
            self.units[u as usize].seen.push((state, line));
            return;
        }
        let id = self.windows.add(text);
        self.by_text.insert(h, id);
        self.units.push(Unit {
            seen: vec![(state, line)],
        });
    }

    fn header(&self, si: usize, sightings: usize) -> String {
        let st = &self.states[si];
        let tr = &self.trajs[st.traj];
        let mut h = format!(
            "Observation {} - trajectory {} of {} (goal: {}; outcome: {}), step {} of {}\nPage: {}\nURL: {}\n",
            st.tag,
            tr.ordinal,
            self.trajs.len(),
            clip(&tr.goal, 240),
            tr.outcome,
            st.step,
            tr.states,
            st.title,
            st.url
        );
        if !st.prev_action.is_empty() {
            h.push_str(&format!("Action that led here: {}\n", st.prev_action));
        }
        if !st.action.is_empty() {
            h.push_str(&format!("Next action taken: {}\n", st.action));
        }
        if !st.thought.is_empty() {
            h.push_str(&format!("Agent thought: {}\n", st.thought));
        }
        if sightings > 1 {
            h.push_str(&format!(
                "Same content seen in {sightings} observations; this is the latest.\n"
            ));
        }
        h
    }

    fn query(&self, text: &str, budget: u64, digest_share: u64) -> Value {
        let q = strip_scaffold(text);
        let mut items: Vec<String> = Vec::new();
        let mut used: u64 = 0;

        // Trajectory digests (workflow and failure knowledge).
        let digest_budget = budget * digest_share / 100;
        let mut dused = 0;
        for (d, _) in self.digests.search(&q, 8) {
            let t = clip_tokens(&self.digest_text[d as usize], 1500);
            let n = tokens::count(&t);
            if dused + n > digest_budget {
                continue;
            }
            dused += n;
            items.push(format!(
                "Workflow digest [src:{}]\n{t}",
                source::encode_path(&self.trajs[d as usize].path)
            ));
        }
        used += dused;

        // Windows grouped by observation; the latest sighting of a unit wins.
        let mut per_state: BTreeMap<usize, (i64, Vec<usize>, usize)> = BTreeMap::new();
        let mut order: Vec<usize> = Vec::new();
        for (u, score) in self.windows.search(&q, 600) {
            let unit = &self.units[u as usize];
            let Some(&(si, line)) = unit.seen.iter().max_by_key(|(si, l)| {
                let st = &self.states[*si];
                (st.traj, st.step, usize::MAX - *l)
            }) else {
                continue;
            };
            let e = per_state.entry(si).or_insert_with(|| {
                order.push(si);
                (score, Vec::new(), 0)
            });
            if line != usize::MAX {
                e.1.push(line);
            }
            e.2 = e.2.max(unit.seen.len());
        }
        order.sort_by(|a, b| per_state[b].0.cmp(&per_state[a].0).then(a.cmp(b)));
        for si in order {
            let (_, hits, sightings) = &per_state[&si];
            let item = self.render(si, hits, *sightings);
            let n = tokens::count(&item);
            if used + n > budget {
                if used + 400 > budget {
                    break;
                }
                continue;
            }
            used += n;
            items.push(item);
        }
        json!({"ok": true, "items": items, "tokens": used})
    }

    fn render(&self, si: usize, hits: &[usize], sightings: usize) -> String {
        let st = &self.states[si];
        let mut out = self.header(si, sightings);
        let n = st.lines.len();
        let mut keep = vec![false; n];
        for k in keep.iter_mut().take(3) {
            *k = true;
        }
        for &h in hits {
            let w = h / WINDOW;
            let from = w.saturating_sub(SPREAD) * WINDOW;
            let to = ((w + 1 + SPREAD) * WINDOW).min(n);
            for k in keep.iter_mut().take(to).skip(from) {
                *k = true;
            }
        }
        out.push_str("Accessibility tree (excerpt):\n");
        let mut gap = false;
        for (i, line) in st.lines.iter().enumerate() {
            if keep[i] {
                out.push_str(line);
                out.push('\n');
                gap = false;
            } else if !gap {
                out.push_str("\t...\n");
                gap = true;
            }
        }
        out
    }
}

/// Drop the answer-format instructions; they carry no retrieval signal.
fn strip_scaffold(q: &str) -> String {
    let lower = q.to_lowercase();
    let cut = [
        "mark your final answer",
        "your final answer should",
        "please provide your answer",
        "wrap your final answer",
    ]
    .iter()
    .filter_map(|m| lower.find(m))
    .min()
    .unwrap_or(q.len());
    q.get(..cut).unwrap_or(q).to_string()
}

fn clip_tokens(text: &str, max: u64) -> String {
    if tokens::count(text) <= max {
        return text.to_string();
    }
    let mut out = String::new();
    for line in text.lines() {
        if tokens::count(&out) + tokens::count(line) > max {
            out.push_str("...\n");
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn main() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    let mut mem = Memory::default();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Err(e) => json!({"ok": false, "error": format!("bad json: {e}")}),
            Ok(req) => match req.get("op").and_then(Value::as_str) {
                Some("insert") => match req.get("trajectory") {
                    Some(t) => mem
                        .insert(t)
                        .unwrap_or_else(|e| json!({"ok": false, "error": e})),
                    None => json!({"ok": false, "error": "insert needs trajectory"}),
                },
                Some("query") => mem.query(
                    req.get("text").and_then(Value::as_str).unwrap_or(""),
                    req.get("budget").and_then(Value::as_u64).unwrap_or(24_000),
                    req.get("digest_share")
                        .and_then(Value::as_u64)
                        .unwrap_or(20)
                        .min(100),
                ),
                Some("stats") => json!({
                    "ok": true,
                    "trajectories": mem.trajs.len(),
                    "states": mem.states.len(),
                    "ir_blocks": mem.blocks,
                    "rule_facts": mem.facts,
                    "units": mem.units.len(),
                    "lexical_model": oo_docs::vector::lexical::MODEL_VERSION,
                }),
                _ => json!({"ok": false, "error": "unknown op"}),
            },
        };
        if writeln!(stdout, "{reply}")
            .and_then(|()| stdout.flush())
            .is_err()
        {
            break;
        }
    }
}
