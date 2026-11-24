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

/// Default lines per indexed window of an accessibility tree (`--window`).
const WINDOW: usize = 20;
/// Default windows of context shown on each side of a hit (`--spread`).
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
    /// Lowercased indexed text, for quoted-phrase matching.
    lower: String,
}

#[derive(Default)]
struct Memory {
    window: usize,
    spread: usize,
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
                let end = (start + self.window).min(n);
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
        let text = strip_bids(text);
        let text = text.as_str();
        let h = blake3::hash(text.as_bytes());
        if let Some(&u) = self.by_text.get(&h) {
            self.units[u as usize].seen.push((state, line));
            return;
        }
        let id = self.windows.add(text);
        self.by_text.insert(h, id);
        self.units.push(Unit {
            seen: vec![(state, line)],
            lower: text.to_lowercase(),
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

    #[allow(clippy::too_many_arguments)]
    fn query(
        &self,
        text: &str,
        budget: u64,
        digest_share: u64,
        per_page: usize,
        agg_sum: bool,
        full_top: usize,
        phrase_boost: i64,
        coverage_note: bool,
        phrase_counts: bool,
        compact: bool,
        alternatives: bool,
    ) -> Value {
        let q = strip_scaffold(text);
        let mut items: Vec<String> = Vec::new();
        let mut used: u64 = 0;
        if coverage_note {
            // What was searched, like the pack compiler's coverage line (D54):
            // a statement about the evidence, not about the answer.
            let note = format!(
                "Memory coverage: {} past task trajectories ({} observed page states) from this environment were searched; the observations below are the closest matches, most relevant first. A tree marked complete lists every element that page showed when it was observed.\n",
                self.trajs.len(),
                self.states.len()
            );
            used += tokens::count(&note);
            items.push(note);
        }

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
        let phrases = quoted_phrases(&q);
        if phrase_counts && !phrases.is_empty() {
            // An exact-text check, like a grep over the memory: how many
            // observed page states contain each quoted phrase. A miss is a
            // fact about the memory, not an answer.
            let mut parts: Vec<String> = Vec::new();
            for p in &phrases {
                let mut states: Vec<usize> = self
                    .units
                    .iter()
                    .filter(|u| u.lower.contains(p.as_str()))
                    .flat_map(|u| u.seen.iter().map(|(si, _)| *si))
                    .collect();
                states.sort_unstable();
                states.dedup();
                parts.push(format!("\"{p}\" in {} of them", states.len()));
            }
            let note = format!(
                "Exact-text check over all {} observed page states (case-insensitive): {}.\n",
                self.states.len(),
                parts.join("; ")
            );
            used += tokens::count(&note);
            items.push(note);
        }
        let mut ranked = self.windows.search(&q, 600);
        if phrase_boost > 0 && !phrases.is_empty() {
            for (u, score) in &mut ranked {
                let lower = &self.units[*u as usize].lower;
                let n = phrases
                    .iter()
                    .filter(|p| lower.contains(p.as_str()))
                    .count() as i64;
                *score += *score * n * phrase_boost / 100;
            }
            ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        }
        for (u, score) in ranked {
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
            if e.1.len() < 3 && agg_sum && score != e.0 {
                e.0 += score / 2;
            }
            e.2 = e.2.max(unit.seen.len());
        }
        order.sort_by(|a, b| per_state[b].0.cmp(&per_state[a].0).then(a.cmp(b)));
        let mut pages: HashMap<String, usize> = HashMap::new();
        let mut full_left = full_top;
        if alternatives {
            // A question that compares named pages ("A vs B", "(a/b/c)",
            // "Among the A, B and C tables") gets the best observation of
            // each one as a complete tree, so every compared page is present.
            for alt in listed_alternatives(&q) {
                // Best-ranked unit first; its latest sighting on a page not
                // shown yet.
                let pick =
                    self.windows
                        .search(&alt, 50)
                        .into_iter()
                        .find_map(|(u, _)| {
                            let unit = &self.units[u as usize];
                            let si =
                                unit.seen.iter().map(|(si, _)| *si).max_by_key(|si| {
                                    (self.states[*si].traj, self.states[*si].step)
                                })?;
                            (!pages.contains_key(&self.page_key(si))).then_some(si)
                        });
                let Some(si) = pick else {
                    continue;
                };
                let item = format!(
                    "Compared item \"{alt}\": best matching page\n{}",
                    self.render(si, &[], 1, true, compact)
                );
                let n = tokens::count(&item);
                if used + n > budget * 3 / 4 {
                    continue;
                }
                used += n;
                items.push(item);
                *pages.entry(self.page_key(si)).or_insert(0) += 1;
            }
        }
        for si in order {
            let (_, hits, sightings) = &per_state[&si];
            let seen = pages.entry(self.page_key(si)).or_insert(0);
            if *seen >= per_page {
                continue;
            }
            // Complete trees go to the best observation of distinct pages, so a
            // page can be checked for what it does not show.
            let full = full_left > 0 && *seen == 0;
            *seen += 1;
            let item = self.render(si, hits, *sightings, full, compact);
            if full {
                full_left -= 1;
            }
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

    /// Page identity: title plus the URL without query string or digits, so
    /// one form seen in many records counts as one page.
    fn page_key(&self, si: usize) -> String {
        let st = &self.states[si];
        let url = st.url.split(['?', '#']).next().unwrap_or("");
        let url: String = url.chars().filter(|c| !c.is_ascii_digit()).collect();
        format!("{}\u{1}{url}", st.title)
    }

    fn render(
        &self,
        si: usize,
        hits: &[usize],
        sightings: usize,
        full: bool,
        compact: bool,
    ) -> String {
        let st = &self.states[si];
        let shown = if compact {
            compact_tree(&st.lines)
        } else {
            st.lines.iter().map(|l| Some(l.clone())).collect()
        };
        let mut out = self.header(si, sightings);
        let n = st.lines.len();
        let mut keep = vec![full; n];
        for k in keep.iter_mut().take(3) {
            *k = true;
        }
        for &h in hits {
            let w = h / self.window;
            let from = w.saturating_sub(self.spread) * self.window;
            let to = ((w + 1 + self.spread) * self.window).min(n);
            for k in keep.iter_mut().take(to).skip(from) {
                *k = true;
            }
        }
        out.push_str(if full {
            "Accessibility tree (complete):\n"
        } else {
            "Accessibility tree (excerpt):\n"
        });
        let mut skipped = 0;
        for (i, line) in shown.iter().enumerate() {
            let Some(line) = line else { continue };
            if keep[i] {
                if skipped > 0 {
                    out.push_str(&format!("\t... ({skipped} lines not shown)\n"));
                    skipped = 0;
                }
                out.push_str(line);
                out.push('\n');
            } else {
                skipped += 1;
            }
        }
        if skipped > 0 {
            out.push_str(&format!("\t... ({skipped} lines not shown)\n"));
        }
        out
    }
}

/// Attributes that never change what a page offers.
const NOISE_ATTRS: &[&str] = &[
    ", clickable",
    ", visible",
    ", focused",
    ", atomic",
    ", live='polite'",
    ", live='assertive'",
    ", relevant='additions text'",
    ", relevant='all'",
];

fn quoted_name(line: &str) -> Option<&str> {
    let a = line.find('\'')?;
    let b = line.rfind('\'')?;
    (b > a).then(|| &line[a + 1..b])
}

fn is_glyph(text: &str) -> bool {
    let t = text.trim();
    (t.len() == 6 && t.starts_with("\\u"))
        || (t.chars().count() == 1 && t.chars().all(|c| ('\u{e000}'..='\u{f8ff}').contains(&c)))
}

/// Compact view of an accessibility tree, one entry per source line
/// (`None` = dropped): element ids and no-op attributes go, and a
/// `StaticText` that only repeats its parent's name (or is an icon glyph)
/// is dropped. Every label, value and state stays.
fn compact_tree(lines: &[String]) -> Vec<Option<String>> {
    let mut parents: Vec<(usize, String)> = Vec::new();
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        let depth = line.len() - line.trim_start_matches('\t').len();
        while parents.last().is_some_and(|(d, _)| *d >= depth) {
            parents.pop();
        }
        let body = strip_bids(line.trim_start_matches('\t'));
        let name = quoted_name(&body).unwrap_or("").to_string();
        if let Some(text) = body.strip_prefix("StaticText ") {
            let text = quoted_name(text).unwrap_or(text);
            let repeats = parents
                .last()
                .is_some_and(|(_, p)| !text.is_empty() && p.contains(text));
            if repeats || is_glyph(text) || text.is_empty() {
                out.push(None);
                continue;
            }
        }
        let mut body = body;
        for a in NOISE_ATTRS {
            body = body.replace(a, "");
        }
        out.push(Some(format!("{}{body}", "\t".repeat(depth))));
        parents.push((depth, name));
    }
    out
}

/// Remove browser element ids (`[112] `, `[a51] `): they differ between
/// sightings of the same page and carry no meaning for retrieval.
fn strip_bids(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('[') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find(']') {
            Some(j)
                if j > 0 && j <= 12 && after[..j].chars().all(|c| c.is_ascii_alphanumeric()) =>
            {
                rest = after[j + 1..].strip_prefix(' ').unwrap_or(&after[j + 1..]);
            }
            _ => {
                out.push('[');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Items a question compares, each as a short search text: a slash list in
/// parentheses ("forms (change request/problem)", each with the word before it),
/// a "vs" chain, or "Among the A, B, and C <noun>" (the shared noun is added
/// to every item). Empty when the question compares nothing.
fn listed_alternatives(q: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    fn push(out: &mut Vec<String>, s: String) {
        let s = s
            .trim()
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_string();
        if s.len() >= 2 && !out.contains(&s) {
            out.push(s);
        }
    }
    // (a/b/c) followed by an optional noun.
    if let (Some(a), Some(b)) = (q.find('('), q.find(')')) {
        let inner = &q[a + 1..b.max(a + 1)];
        if inner.contains('/') {
            let noun = q[..a].split_whitespace().last().unwrap_or("");
            let noun = if noun.chars().all(char::is_alphabetic) {
                noun
            } else {
                ""
            };
            for it in inner.split('/') {
                push(&mut out, format!("{it} {noun}"));
            }
        }
    }
    // A vs B vs C, inside one sentence or clause.
    let vs_from = out.len();
    let normalized = q.replace(" vs. ", " vs ");
    for clause in normalized.split(['.', '?', ':', ';', '\n']) {
        if !clause.contains(" vs") {
            continue;
        }
        let parts: Vec<&str> = clause.split(" vs").collect();
        for (i, part) in parts.iter().enumerate() {
            let words: Vec<&str> = part.split_whitespace().collect();
            // The first item is the tail of the clause before " vs".
            let take = words.len().min(4);
            let slice = if i == 0 {
                &words[words.len() - take..]
            } else {
                &words[..take]
            };
            push(&mut out, slice.join(" "));
        }
    }
    let vs_to = out.len();
    // Among the A, B, and C noun ...
    if let Some(i) = q.find("Among the ").or_else(|| q.find("Among these ")) {
        let rest = &q[i..];
        let rest = &rest[rest.find(' ').map_or(0, |k| k + 1)..];
        let rest = &rest[rest.find(' ').map_or(0, |k| k + 1)..];
        let end = rest.find(['?', '.']).unwrap_or(rest.len());
        let span = &rest[..end];
        let span = match span
            .find(", which")
            .or_else(|| span.find(", how"))
            .or_else(|| span.find(", what"))
        {
            Some(k) => &span[..k],
            None => span,
        };
        if span.contains(',') && span.contains(" and ") {
            let items: Vec<String> = span
                .replace(", and ", ", ")
                .replace(" and ", ", ")
                .split(", ")
                .map(str::to_string)
                .collect();
            let last = items.last().cloned().unwrap_or_default();
            let tail: Vec<&str> = last.split_whitespace().skip(1).collect();
            let tail = tail.join(" ");
            for (k, it) in items.iter().enumerate() {
                if k + 1 == items.len() {
                    push(&mut out, it.clone());
                } else {
                    push(&mut out, format!("{it} {tail}"));
                }
            }
        }
    }
    // "vs" items name no page kind; they get the one the question uses.
    let lower = q.to_lowercase();
    if let Some(noun) = ["form", "page", "table", "view", "list", "tab"]
        .iter()
        .find(|n| lower.contains(&format!(" {n}")))
    {
        for it in &mut out[vs_from..vs_to] {
            if !it.to_lowercase().contains(noun) {
                it.push(' ');
                it.push_str(noun);
            }
        }
    }
    out.truncate(6);
    out
}

/// Lowercased spans the question quotes ("..", `..`, \u{201c}..\u{201d}), at
/// least 3 characters: field labels and button names worth matching exactly.
fn quoted_phrases(q: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (open, close) in [('"', '"'), ('`', '`'), ('\u{201c}', '\u{201d}')] {
        let mut rest = q;
        while let Some(i) = rest.find(open) {
            let after = &rest[i + open.len_utf8()..];
            let Some(j) = after.find(close) else { break };
            let p = after[..j].trim().to_lowercase();
            if p.chars().count() >= 3 && p.chars().count() <= 80 && !out.contains(&p) {
                out.push(p);
            }
            rest = &after[j + close.len_utf8()..];
        }
    }
    out
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
    let mut mem = Memory {
        window: WINDOW,
        spread: SPREAD,
        ..Memory::default()
    };
    let argv: Vec<String> = std::env::args().skip(1).collect();
    for pair in argv.chunks(2) {
        let v = pair.get(1).and_then(|v| v.parse::<usize>().ok());
        match (pair[0].as_str(), v) {
            ("--window", Some(v)) if v > 0 => mem.window = v,
            ("--spread", Some(v)) => mem.spread = v,
            _ => {
                eprintln!("usage: lme_v2_memory [--window N] [--spread N]");
                std::process::exit(2);
            }
        }
    }
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
                    req.get("per_page")
                        .and_then(Value::as_u64)
                        .map_or(usize::MAX, |n| usize::try_from(n).unwrap_or(usize::MAX)),
                    req.get("agg_sum").and_then(Value::as_bool).unwrap_or(false),
                    req.get("full_top")
                        .and_then(Value::as_u64)
                        .map_or(0, |n| usize::try_from(n).unwrap_or(0)),
                    req.get("phrase_boost")
                        .and_then(Value::as_i64)
                        .unwrap_or(0)
                        .clamp(0, 1000),
                    req.get("coverage_note")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    req.get("phrase_counts")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    req.get("compact").and_then(Value::as_bool).unwrap_or(false),
                    req.get("alternatives")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn trajectory(id: &str, tree: &str) -> Value {
        json!({
            "id": id,
            "goal": "Open the incident list",
            "outcome": "success",
            "states": [
                {"url": "https://x.test/a", "action": "click('12')", "thought": "open it",
                 "accessibility_tree": format!("RootWebArea 'Incidents | Portal'\n\t[12] link 'Incidents'\n{tree}")},
                {"url": "https://x.test/b?sys_id=9", "action": null, "thought": "",
                 "accessibility_tree": "RootWebArea 'Incident list'\n\t[40] button 'Filters'"}
            ]
        })
    }

    #[test]
    fn compact_tree_keeps_labels_and_drops_repeats() {
        let lines: Vec<String> = [
            "RootWebArea 'Incident', focused",
            "\t[b56] link 'Back', clickable, visible",
            "\t\tStaticText '\\uf132'",
            "\t\tStaticText 'Back'",
            "\t[b62] combobox 'State', clickable, visible, expanded=False",
            "\t\tStaticText 'New'",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
        let c = compact_tree(&lines);
        assert_eq!(c[0].as_deref(), Some("RootWebArea 'Incident'"));
        assert_eq!(c[1].as_deref(), Some("\tlink 'Back'"));
        assert_eq!(c[2], None);
        assert_eq!(c[3], None);
        assert_eq!(c[4].as_deref(), Some("\tcombobox 'State', expanded=False"));
        assert_eq!(c[5].as_deref(), Some("\t\tStaticText 'New'"));
    }

    #[test]
    fn compared_items_are_found() {
        assert_eq!(
            listed_alternatives("Among these five forms (change request/problem/user), which one?"),
            vec!["change request forms", "problem forms", "user forms"]
        );
        assert_eq!(
            listed_alternatives(
                "I use forms. Create Change Request vs Problem vs Incident. Which?"
            ),
            vec![
                "Create Change Request form",
                "Problem form",
                "Incident form"
            ]
        );
        assert_eq!(
            listed_alternatives("Create change request vs. create hardware asset: how many tabs?"),
            vec!["Create change request tab", "create hardware asset tab"]
        );
        assert_eq!(
            listed_alternatives(
                "Among the Orders, Products, and Pages grid tables, which tables show Export?"
            ),
            vec![
                "Orders grid tables",
                "Products grid tables",
                "Pages grid tables"
            ]
        );
        assert!(listed_alternatives("Which button submits the form?").is_empty());
    }

    #[test]
    fn helpers() {
        assert_eq!(
            strip_bids("\t[12] link 'A' [a51] b [not an id] [ x]"),
            "\tlink 'A' b [not an id] [ x]"
        );
        assert_eq!(
            strip_scaffold("Which one?\n\nMark your final answer in \\boxed{}."),
            "Which one?\n\n"
        );
        assert_eq!(
            title_of("RootWebArea 'Incidents | Portal', focused\n..."),
            "Incidents | Portal"
        );
        assert_eq!(
            resolve_action("click('12')", "x\n\t[12] link 'Incidents'"),
            "click('12') on [12] link 'Incidents'"
        );
        assert_eq!(resolve_action("scroll(0, 200)", "x"), "scroll(0, 200)");
    }

    #[test]
    fn latest_sighting_wins_and_queries_replay() {
        let mut mem = Memory {
            window: 4,
            spread: 1,
            ..Memory::default()
        };
        mem.insert(&trajectory("t1", "\t[13] checkbox 'Depreciation'"))
            .unwrap();
        mem.insert(&trajectory("t2", "\t[99] checkbox 'Depreciation'"))
            .unwrap();
        assert_eq!(mem.trajs.len(), 2);
        // Same text up to element ids is one unit seen twice.
        let q = mem.query(
            "Which checkbox mentions Depreciation?",
            4000,
            0,
            usize::MAX,
            false,
            0,
            0,
            false,
            false,
            false,
            false,
        );
        let items = q["items"].as_array().unwrap();
        let first = items[0].as_str().unwrap();
        assert!(first.contains("trajectory 2 of 2"), "{first}");
        assert!(first.contains("seen in 2 observations"));
        assert!(first.contains("[src:trajectories/0002-t2.md#L"));
        assert_eq!(
            mem.query(
                "Which checkbox mentions Depreciation?",
                4000,
                0,
                usize::MAX,
                false,
                0,
                0,
                false,
                false,
                false,
                false
            ),
            q
        );
        // The budget bounds the reply.
        let small = mem.query(
            "Depreciation",
            10,
            0,
            usize::MAX,
            false,
            0,
            0,
            false,
            false,
            false,
            false,
        );
        assert!(small["items"].as_array().unwrap().is_empty());
        // The exact-text check reports hits and misses as counts.
        let check = mem.query(
            "Is there a \"Depreciation\" or `Amortization` checkbox?",
            4000,
            0,
            usize::MAX,
            false,
            0,
            0,
            false,
            true,
            false,
            false,
        );
        let note = check["items"][0].as_str().unwrap();
        assert!(note.contains("\"depreciation\" in 2 of them"), "{note}");
        assert!(note.contains("\"amortization\" in 0 of them"), "{note}");
    }
}
