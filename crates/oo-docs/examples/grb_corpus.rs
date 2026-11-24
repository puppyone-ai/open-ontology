//! GraphRAG-Bench corpus retrieval on the oo-docs engine (benchmark driver,
//! not shipped). Started with `--docs docs.jsonl` (one `{"path","text"}`
//! per line, written by `tools/oo-bench/graphrag_bench/run_grb.py`), then
//! spoken to over stdio, one JSON object per line.
//!
//! Load: every document is markdown, parsed and normalized into
//! `oo.doc-ir/v1` by the engine and run through the `rules/1` extraction.
//! Consecutive IR blocks form passages of about `--words` words; passages
//! are indexed in the `bm25/1` channel. Names (capitalized word runs; the
//! rules extraction finds almost nothing in prose and is only counted),
//! keyed by the resolver's canonical `norm`, become entity nodes linked to the passages that mention them, so
//! the corpus is a passage-entity graph.
//!
//! * `{"op":"query","text":..,"k":N,"spread":S}`: names in the question
//!   that are entities of the corpus are the anchors; `walk/1` ranks
//!   passages by the anchors they mention, BM25 breaking ties; the best `k`
//!   passages come back with `S` neighbor passages on each side, merged, in
//!   document order, each with its provenance tag (`path#anchor`). Never an
//!   answer.
//! * `{"op":"stats"}`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{BufRead, Write};

use oo_docs::compile::source;
use oo_docs::extract::rules;
use oo_docs::parse::markdown::parse_markdown;
use oo_docs::resolve::normalize::norm;
use oo_docs::vector::lexical::{Index, terms};
use oo_docs::vector::walk::{self, Anchor, Graph};
use serde_json::{Value, json};

/// Weight of one anchor reached against the BM25 score (Q16).
const ANCHOR_W: i64 = 1 << 19;
/// A name in more passages than this is too common to anchor on.
const MAX_DF: usize = 400;

struct Passage {
    doc: usize,
    tag: String,
    text: String,
}

#[derive(Default)]
struct Corpus {
    paths: Vec<String>,
    passages: Vec<Passage>,
    index: Index,
    /// Entity key -> entity node id (node ids start after the passages).
    entities: HashMap<String, u32>,
    entity_df: Vec<usize>,
    graph: Graph,
    blocks: usize,
    facts: usize,
    mentions: usize,
    rule_entities: usize,
}

/// Runs of capitalized words ("Sir John Arundel", "St. Michael's Mount").
fn names(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut run: Vec<&str> = Vec::new();
    let flush = |run: &mut Vec<&str>, out: &mut Vec<String>| {
        if !run.is_empty() {
            let s = run.join(" ");
            if s.chars().filter(|c| c.is_alphabetic()).count() >= 3 {
                out.push(s);
            }
            run.clear();
        }
    };
    for raw in text.split_whitespace() {
        let w = raw.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '.');
        let w = w.trim_end_matches('.').trim_end_matches("'s");
        let cap = w.chars().next().is_some_and(char::is_uppercase)
            && w.chars().skip(1).any(char::is_lowercase);
        if cap && !terms(w).is_empty() {
            run.push(w);
        } else {
            flush(&mut run, &mut out);
        }
        if raw.ends_with(['.', ',', ';', ':', '!', '?', '"', ')']) {
            flush(&mut run, &mut out);
        }
    }
    flush(&mut run, &mut out);
    out
}

impl Corpus {
    fn load(path: &str, words: usize) -> Result<Self, String> {
        let mut c = Corpus::default();
        let f = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
        for line in std::io::BufReader::new(f).lines() {
            let line = line.map_err(|e| e.to_string())?;
            let v: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            let p = v
                .get("path")
                .and_then(Value::as_str)
                .ok_or("doc without path")?
                .to_string();
            let text = v.get("text").and_then(Value::as_str).unwrap_or("");
            let parsed = parse_markdown(text.as_bytes()).map_err(|e| format!("parse {p}: {e}"))?;
            let ir = &parsed.ir;
            let x = rules::extract(&p, ir);
            c.blocks += ir.blocks.len();
            c.facts += x.facts.len();
            c.rule_entities += x.entities.len();
            let di = c.paths.len();
            c.paths.push(p.clone());
            let mut cur: Vec<&str> = Vec::new();
            let mut tag = String::new();
            let mut n = 0;
            let blocks: Vec<_> = ir.blocks.iter().collect();
            for (bi, b) in blocks.iter().enumerate() {
                if cur.is_empty() {
                    tag = source::tag(&p, &b.anchor);
                }
                cur.push(b.text.as_str());
                n += b.text.split_whitespace().count();
                if n >= words || bi + 1 == blocks.len() {
                    c.passages.push(Passage {
                        doc: di,
                        tag: tag.clone(),
                        text: cur.join("\n"),
                    });
                    cur.clear();
                    n = 0;
                }
            }
        }
        // Passage index and passage-entity edges.
        let np = c.passages.len();
        let mut keyed: Vec<Vec<String>> = Vec::with_capacity(np);
        for p in &c.passages {
            c.index.add(&p.text);
            let ks: BTreeSet<String> = names(&p.text).iter().map(|n| norm(n)).collect();
            keyed.push(ks.into_iter().collect());
        }
        let mut next = u32::try_from(np).map_err(|e| e.to_string())?;
        let mut ids: HashMap<String, u32> = HashMap::new();
        let mut df: Vec<usize> = Vec::new();
        let mut edges: Vec<(u32, u32)> = Vec::new();
        for (pi, ks) in keyed.iter().enumerate() {
            for k in ks {
                let id = *ids.entry(k.clone()).or_insert_with(|| {
                    let id = next;
                    next += 1;
                    df.push(0);
                    id
                });
                df[(id as usize) - np] += 1;
                edges.push((u32::try_from(pi).unwrap_or(0), id));
            }
        }
        c.graph = Graph::new(next as usize);
        for (p, e) in &edges {
            c.graph.add_edge(*p, *e, 0);
        }
        c.graph.seal();
        c.mentions = edges.len();
        c.entities = ids;
        c.entity_df = df;
        Ok(c)
    }

    fn query(&self, q: &Value) -> Value {
        let text = q.get("text").and_then(Value::as_str).unwrap_or("");
        let k = q.get("k").and_then(Value::as_u64).unwrap_or(5) as usize;
        let spread = q.get("spread").and_then(Value::as_u64).unwrap_or(0) as usize;
        let np = self.passages.len();
        let mut anchors = Vec::new();
        let mut used = Vec::new();
        for n in names(text) {
            let key = norm(&n);
            if let Some(&id) = self.entities.get(&key) {
                let df = self.entity_df[id as usize - np];
                if df <= MAX_DF {
                    anchors.push(Anchor {
                        nodes: vec![id],
                        relations: vec![],
                    });
                    used.push(json!({"name": n, "passages": df}));
                }
            }
        }
        let bm: BTreeMap<u32, i64> = self.index.search(text, 2000).into_iter().collect();
        let ranked = walk::rank(
            &self.graph,
            &anchors,
            |id| (id as usize) < np,
            &bm,
            ANCHOR_W,
            k,
        );
        // Expand with neighbors in the same document, merge, keep order.
        let mut pick: BTreeSet<usize> = BTreeSet::new();
        for (id, _) in &ranked {
            let i = *id as usize;
            for j in i.saturating_sub(spread)..=(i + spread).min(np - 1) {
                if self.passages[j].doc == self.passages[i].doc {
                    pick.insert(j);
                }
            }
        }
        let hits: Vec<Value> = pick
            .iter()
            .map(|i| {
                let p = &self.passages[*i];
                json!({"tag": p.tag, "path": self.paths[p.doc], "text": p.text})
            })
            .collect();
        json!({"ok": true, "passages": hits, "anchors": used, "top": ranked.iter().map(|x| x.0).collect::<Vec<_>>()})
    }
}

fn arg(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(docs) = arg(&args, "--docs") else {
        eprintln!("usage: grb_corpus --docs docs.jsonl [--words N]");
        std::process::exit(2);
    };
    let words = arg(&args, "--words")
        .and_then(|w| w.parse().ok())
        .unwrap_or(200);
    let c = match Corpus::load(&docs, words) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("load failed: {e}");
            std::process::exit(1);
        }
    };
    eprintln!(
        "loaded {} docs, {} IR blocks, {} rule facts, {} rule entities, {} passages, {} entities, {} mentions",
        c.paths.len(),
        c.blocks,
        c.facts,
        c.rule_entities,
        c.passages.len(),
        c.entities.len(),
        c.mentions
    );
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Err(e) => json!({"ok": false, "error": format!("bad json: {e}")}),
            Ok(q) => match q.get("op").and_then(Value::as_str) {
                Some("query") => c.query(&q),
                Some("stats") => json!({
                    "ok": true, "docs": c.paths.len(), "blocks": c.blocks, "facts": c.facts,
                    "passages": c.passages.len(), "entities": c.entities.len(), "mentions": c.mentions,
                }),
                _ => json!({"ok": false, "error": "unknown op"}),
            },
        };
        if writeln!(out, "{reply}").and_then(|()| out.flush()).is_err() {
            break;
        }
    }
}
