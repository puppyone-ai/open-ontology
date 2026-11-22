#!/usr/bin/env python3
"""Deterministic generator of the profile test set (D89): a synthetic engineering repo, three papers, the frozen gold and 10 questions.

    python3 -B tools/super-ontology/profiles/gen.py [--out DIR] [--check]

Stdlib only. No clocks, no random. Writes <out>/project/harborq/ (the repo), <out>/project/papers/*.pdf (text layer PDFs written by the golden
set's writer, the way tools/super-ontology/acceptance/gen.py writes its PDFs), <out>/gold/{instruction,code,paper}.jsonl,
<out>/gold/questions.jsonl, <out>/README.md and MANIFEST.sha256. All content is synthetic. The gold is written down here, next to the text it
describes, and is frozen before any run (it is never edited after a run).
"""
import argparse, hashlib, json, sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "golden"))
import writers as W  # noqa: E402

FILES = {}
GOLD = {"instruction": [], "code": [], "paper": []}


def put(path, text):
    FILES[path] = text.encode("utf-8") if isinstance(text, str) else text


def gold(kind, file, label, name, **extra):
    GOLD[kind].append({"file": file, "label": label, "name": name, **extra})


# ------------------------------------------------------------------ instruction documents
def instruction_doc(path, text, rules=(), commands=(), tools=(), paths=()):
    put(path, text)
    for r in rules:
        gold("instruction", path, "Rule", r)
    for c in commands:
        gold("instruction", path, "Command", c)
    for t in tools:
        gold("instruction", path, "Tool", t)
    for p in paths:
        gold("instruction", path, "Path", p)


AGENTS = """# AGENTS.md

Harbor Queue is a small job queue library with a Rust core, Python tools and a TypeScript dashboard.

## Setup

- Install the toolchain with rustup, then run pnpm install inside web/.
- The Python tools need Python 3.11.

## Commands

```bash
cargo build --workspace
cargo test -p harborq-core
cargo fmt --all
pnpm --dir web test
python3 tools/report.py --since 7d
```

## Rules

- Never edit files under core/src/generated/ by hand; run make generate instead.
- Run the unit tests with cargo test -p harborq-core before every commit.
- Keep public functions documented with a doc comment.
- Do not add a dependency without an ADR in docs/adr/.
- Use cargo fmt --all before committing Rust code; CI rejects unformatted code.
- Prefer small pull requests that change one thing.
- Web code must pass pnpm --dir web test and eslint.

## Architecture notes

- The queue stores jobs in SQLite (see docs/adr/0001-use-sqlite-for-storage.md).
- Workers poll core/src/worker.rs every 200 ms.
- The dashboard in web/ reads the same database through a read-only API.

Owner: Platform Team
"""
RULES_AGENTS = [
    "Never edit files under core/src/generated/ by hand; run make generate instead.",
    "Run the unit tests with cargo test -p harborq-core before every commit.",
    "Keep public functions documented with a doc comment.",
    "Do not add a dependency without an ADR in docs/adr/.",
    "Use cargo fmt --all before committing Rust code; CI rejects unformatted code.",
    "Prefer small pull requests that change one thing.",
    "Web code must pass pnpm --dir web test and eslint.",
]
instruction_doc(
    "harborq/AGENTS.md", AGENTS, RULES_AGENTS,
    commands=["cargo build --workspace", "cargo test -p harborq-core", "cargo fmt --all", "pnpm --dir web test", "python3 tools/report.py --since 7d", "pnpm install", "make generate"],
    tools=["rustup", "pnpm", "cargo", "python3", "make", "eslint"],
    paths=["web", "core/src/generated", "docs/adr", "core/src/worker.rs", "docs/adr/0001-use-sqlite-for-storage.md", "tools/report.py"],
)

README = """# Harbor Queue

Harbor Queue is a durable job queue for small services. Jobs live in one SQLite file, workers pull them in order, and failed jobs retry with backoff.

## Quick start

```bash
cargo add harborq-core
pip install harborq-tools
```

```rust
let queue = Queue::open("jobs.db")?;
queue.push("send-email", payload)?;
```

## Usage notes

Always call Queue::close before the process exits so pending writes are flushed.

The Python tools in tools/ export and report on the queue. The dashboard in web/ shows queue depth.

## Contributing

See CONTRIBUTING.md. The license is Apache-2.0.
"""
instruction_doc(
    "harborq/README.md", README, ["Always call Queue::close before the process exits so pending writes are flushed."],
    commands=["cargo add harborq-core", "pip install harborq-tools"], tools=["cargo", "pip"], paths=["tools", "web", "CONTRIBUTING.md"],
)

CONTRIBUTING = """# Contributing

Thanks for helping. Harbor Queue is maintained by a small team.

## Workflow

1. Fork the repository and create a branch with git checkout -b my-change.
2. Make one focused change and add a test for it.
3. Run cargo test --workspace and pnpm --dir web test.
4. Open a pull request that links the issue.

## Pull request rules

- Commit messages must use the imperative mood.
- Every pull request needs at least one reviewer from the Platform Team.
- Do not force-push to a branch that already has review comments.
- Update CHANGELOG.md for every user visible change.

## Where things live

The Rust core is in core/src/. Python tools are in tools/. The dashboard is in web/src/.
"""
instruction_doc(
    "harborq/CONTRIBUTING.md", CONTRIBUTING,
    ["Commit messages must use the imperative mood.", "Every pull request needs at least one reviewer from the Platform Team.",
     "Do not force-push to a branch that already has review comments.", "Update CHANGELOG.md for every user visible change."],
    commands=["git checkout -b my-change", "cargo test --workspace", "pnpm --dir web test"], tools=["git", "cargo", "pnpm"],
    paths=["CHANGELOG.md", "core/src", "tools", "web/src"],
)

STYLE = """# Style guide

## Naming rules

- Name queues with lowercase kebab-case.
- Error types end with Error.
- Public items need doc comments.

## Formatting

- Rust code is formatted by rustfmt with the default settings.
- Python code follows black with a line length of 100.
- TypeScript uses prettier; run pnpm --dir web format to fix a file.

## Background

The style guide was written in 2025 after the first external contribution.
"""
instruction_doc(
    "harborq/docs/STYLE_GUIDE.md", STYLE,
    ["Name queues with lowercase kebab-case.", "Error types end with Error.", "Public items need doc comments.",
     "Rust code is formatted by rustfmt with the default settings.", "Python code follows black with a line length of 100.",
     "TypeScript uses prettier; run pnpm --dir web format to fix a file."],
    commands=["pnpm --dir web format"], tools=["rustfmt", "black", "prettier", "pnpm"], paths=[],
)

ADR1 = """# 1. Use SQLite for storage

Status: Accepted

## Context

Harbor Queue targets small services that cannot run a separate database. Operators want a single file they can back up.

## Decision

We will use SQLite as the only storage engine, opened in WAL mode.

## Consequences

- Deployment is a single file next to the binary.
- Writers are serialised, so bulk inserts go through the batch API in core/src/queue.rs.
"""
ADR2 = """# 2. Retry failed jobs with exponential backoff

Status: Accepted

## Context

Transient failures are common when workers call external services. A fixed delay either wastes time or overloads the service.

## Decision

Workers retry a failed job with exponential backoff and jitter. The first delay is 2 seconds and each delay doubles.

## Consequences

- A job is retried at most 5 times and then moved to the dead letter table.
- The delay is computed by next_delay in core/src/retry.rs.
- Workers must not retry a job that failed with a validation error.
"""
instruction_doc("harborq/docs/adr/0001-use-sqlite-for-storage.md", ADR1, [], paths=["core/src/queue.rs"])
instruction_doc("harborq/docs/adr/0002-retry-with-backoff.md", ADR2, ["Workers must not retry a job that failed with a validation error."], paths=["core/src/retry.rs"])

put("harborq/CHANGELOG.md", "# Changelog\n\n## 0.3.0\n\n- Added the dead letter table.\n- Fixed a race in worker shutdown.\n\n## 0.2.0\n\n- Added exponential backoff.\n")
put("harborq/LICENSE", "Apache License\nVersion 2.0, January 2004\nhttp://www.apache.org/licenses/\n")
put("harborq/ops/worker.log", "2027-02-01T10:00:01Z worker started\n2027-02-01T10:00:02Z job 17 done\n2027-02-01T10:00:09Z job 18 failed attempt=1\n")

# ------------------------------------------------------------------ code
RUST_LIB = """//! Harbor Queue core.
use std::collections::HashMap;

pub mod queue;
pub mod retry;
pub mod worker;

/// Settings of a queue.
pub struct Config {
    pub path: String,
    pub max_attempts: u32,
}

pub enum Status {
    Pending,
    Running,
    Dead,
}

pub trait Store {
    fn load(&self, id: u64) -> Option<String>;
}

pub fn default_config() -> Config {
    // fn commented_out() {}
    let note = "fn not_a_function() {}";
    let _ = (note, HashMap::<u32, u32>::new());
    Config { path: "jobs.db".into(), max_attempts: 5 }
}

#[cfg(test)]
mod tests {
    fn hidden_helper() {}
}
"""
put("harborq/core/src/lib.rs", RUST_LIB)
for n in ["Config", "Status", "Store"]:
    gold("code", "harborq/core/src/lib.rs", "Type", f"harborq/core/src/lib.rs::{n}")
gold("code", "harborq/core/src/lib.rs", "Function", "harborq/core/src/lib.rs::default_config")
gold("code", "harborq/core/src/lib.rs", "Function", "harborq/core/src/lib.rs::Store::load")

RUST_QUEUE = """use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum QueueError {
    #[error("storage failed")]
    Storage,
}

#[derive(Serialize, Deserialize)]
pub struct Job {
    pub id: u64,
    pub kind: String,
}

pub struct Queue {
    conn: Connection,
}

impl Queue {
    pub fn open(path: &str) -> Result<Queue, QueueError> {
        let conn = Connection::open(path).map_err(|_| QueueError::Storage)?;
        Ok(Queue { conn })
    }

    pub fn push(&self, kind: &str) -> Result<u64, QueueError> {
        fn inner() {}
        inner();
        let _ = (&self.conn, kind);
        Ok(1)
    }

    pub fn close(self) {}
}

pub fn batch_insert(queue: &Queue, kinds: &[&str]) -> usize {
    kinds.iter().filter(|k| queue.push(k).is_ok()).count()
}
"""
put("harborq/core/src/queue.rs", RUST_QUEUE)
for n in ["QueueError", "Job", "Queue"]:
    gold("code", "harborq/core/src/queue.rs", "Type", f"harborq/core/src/queue.rs::{n}")
for n in ["Queue::open", "Queue::push", "Queue::close", "batch_insert"]:
    gold("code", "harborq/core/src/queue.rs", "Function", f"harborq/core/src/queue.rs::{n}")
for d in ["rusqlite", "serde", "thiserror"]:
    gold("code", "harborq/core/src/queue.rs", "Dependency", d)

RUST_RETRY = """use std::time::Duration;

use rand::Rng;

pub struct Backoff {
    pub base_ms: u64,
    pub max_attempts: u32,
}

impl Backoff {
    pub fn new() -> Backoff {
        Backoff { base_ms: 2000, max_attempts: 5 }
    }
}

pub fn next_delay(attempt: u32, backoff: &Backoff) -> Duration {
    let exp = backoff.base_ms << attempt.min(10);
    let jitter = rand::thread_rng().gen_range(0..250);
    Duration::from_millis(exp + jitter)
}

pub fn should_retry(attempt: u32, backoff: &Backoff) -> bool {
    attempt < backoff.max_attempts
}
"""
put("harborq/core/src/retry.rs", RUST_RETRY)
gold("code", "harborq/core/src/retry.rs", "Type", "harborq/core/src/retry.rs::Backoff")
for n in ["Backoff::new", "next_delay", "should_retry"]:
    gold("code", "harborq/core/src/retry.rs", "Function", f"harborq/core/src/retry.rs::{n}")
gold("code", "harborq/core/src/retry.rs", "Dependency", "rand")

RUST_WORKER = """use tokio::time::sleep;
use tokio::sync::mpsc;

use crate::queue::Queue;
use crate::retry::next_delay;

pub struct Worker {
    pub name: String,
}

impl Worker {
    pub async fn run(&self, queue: &Queue) {
        let _ = (queue, next_delay);
        sleep(std::time::Duration::from_millis(200)).await;
    }
}

pub fn spawn_all(count: usize) -> Vec<Worker> {
    let (_tx, _rx) = mpsc::channel::<u8>(1);
    (0..count).map(|i| Worker { name: format!("w{i}") }).collect()
}
"""
put("harborq/core/src/worker.rs", RUST_WORKER)
gold("code", "harborq/core/src/worker.rs", "Type", "harborq/core/src/worker.rs::Worker")
for n in ["Worker::run", "spawn_all"]:
    gold("code", "harborq/core/src/worker.rs", "Function", f"harborq/core/src/worker.rs::{n}")
gold("code", "harborq/core/src/worker.rs", "Dependency", "tokio")

PY_EXPORT = '''"""Export the queue to CSV."""
import csv
import sqlite3
import click
import requests
from rich.console import Console

console = Console()


class Exporter:
    def __init__(self, path):
        self.path = path

    def rows(self):
        db = sqlite3.connect(self.path)
        return db.execute("select id, kind from jobs").fetchall()

    def write(self, out):
        def helper(row):
            return row

        with open(out, "w") as f:
            csv.writer(f).writerows(helper(r) for r in self.rows())


@click.command()
def main():
    """def fake_function(): pass"""
    Exporter("jobs.db").write("jobs.csv")
    requests.post("https://example.invalid/hook")
    console.print("done")


def summarize(rows):
    return len(rows)
'''
put("harborq/tools/export.py", PY_EXPORT)
gold("code", "harborq/tools/export.py", "Class", "harborq/tools/export.py::Exporter")
for n in ["Exporter.__init__", "Exporter.rows", "Exporter.write", "main", "summarize"]:
    gold("code", "harborq/tools/export.py", "Function", f"harborq/tools/export.py::{n}")
for d in ["click", "requests", "rich"]:
    gold("code", "harborq/tools/export.py", "Dependency", d)

PY_REPORT = '''import argparse
import json
from datetime import datetime
import pandas as pd


def load(path):
    with open(path) as f:
        return json.load(f)


def depth_by_day(frame):
    return frame.groupby("day").size()


def parse_args():
    p = argparse.ArgumentParser()
    p.add_argument("--since", default="7d")
    return p.parse_args()


if __name__ == "__main__":
    args = parse_args()
    print(depth_by_day(pd.DataFrame(load("jobs.json"))), datetime.now())
'''
put("harborq/tools/report.py", PY_REPORT)
for n in ["load", "depth_by_day", "parse_args"]:
    gold("code", "harborq/tools/report.py", "Function", f"harborq/tools/report.py::{n}")
gold("code", "harborq/tools/report.py", "Dependency", "pandas")

TS_API = """import { z } from "zod";
import React from "react";
import { helper } from "./helper";

export interface QueueStats {
  depth: number;
  oldestMs: number;
}

export type QueueName = string;

export enum Health {
  Ok,
  Late,
}

export const statsSchema = z.object({ depth: z.number() });

export async function fetchStats(name: QueueName): Promise<QueueStats> {
  const response = await fetch(`/api/queues/${name}`);
  return statsSchema.parse(await response.json()) as QueueStats;
}

export const useDepth = (stats: QueueStats) => {
  return helper(stats.depth);
};

export class Poller {
  start() {
    return React.version;
  }
}
"""
put("harborq/web/src/api.ts", TS_API)
for n in ["QueueStats", "QueueName", "Health"]:
    gold("code", "harborq/web/src/api.ts", "Type", f"harborq/web/src/api.ts::{n}")
for n in ["fetchStats", "useDepth", "Poller.start"]:
    gold("code", "harborq/web/src/api.ts", "Function", f"harborq/web/src/api.ts::{n}")
gold("code", "harborq/web/src/api.ts", "Class", "harborq/web/src/api.ts::Poller")
for d in ["zod", "react"]:
    gold("code", "harborq/web/src/api.ts", "Dependency", d)
# statsSchema is a constant, not a function: not gold, and not expected.

put("harborq/Cargo.toml", '[package]\nname = "harborq-core"\nversion = "0.3.0"\n\n[dependencies]\nrusqlite = "0.31"\nserde = { version = "1", features = ["derive"] }\ntokio = { version = "1", features = ["rt"] }\nthiserror = "1"\nrand = "0.8"\n\n[dev-dependencies]\nproptest = "1"\n')
gold("code", "harborq/Cargo.toml", "Package", "harborq-core")
for d in ["rusqlite", "serde", "tokio", "thiserror", "rand", "proptest"]:
    gold("code", "harborq/Cargo.toml", "Dependency", d)
put("harborq/pyproject.toml", '[project]\nname = "harborq-tools"\nversion = "0.3.0"\ndependencies = [\n  "click>=8",\n  "requests",\n  "rich",\n  "pandas",\n]\n')
gold("code", "harborq/pyproject.toml", "Package", "harborq-tools")
for d in ["click", "requests", "rich", "pandas"]:
    gold("code", "harborq/pyproject.toml", "Dependency", d)
put("harborq/web/package.json", json.dumps({"name": "@harborq/web", "version": "0.3.0", "dependencies": {"react": "^19.0.0", "zod": "^4.0.0"}, "devDependencies": {"vitest": "^4.0.0", "eslint": "^10.0.0"}}, indent=2) + "\n")
gold("code", "harborq/web/package.json", "Package", "@harborq/web")
for d in ["react", "zod", "vitest", "eslint"]:
    gold("code", "harborq/web/package.json", "Dependency", d)

# ------------------------------------------------------------------ papers
def paper(path, title, authors, venue_line, abstract, body, refs, extra_lines=(), zh=False, gold_items=()):
    blocks = [("h1", title, "title"), ("p", authors, "authors"), ("p", venue_line, "venue")]
    blocks += [("p", x, "front") for x in extra_lines]
    blocks.append(("h2", "摘要" if zh else "Abstract", "abstract-h"))
    blocks.append(("p", abstract, "abstract"))
    for h, t in body:
        blocks.append(("h2", h, "sec"))
        blocks.append(("p", t, "body"))
    blocks.append(("h2", "参考文献" if zh else "References", "refs-h"))
    for r in refs:
        blocks.append(("p", r, "ref"))
    d = {"title": title, "blocks": blocks, "fm": {}, "created": "2027-01-12"}
    data, _ = W.write_pdf(d)
    data = narrow_latin(data)
    put(path, data)
    for label, name, *rest in gold_items:
        gold("paper", path, label, name, **(rest[0] if rest else {}))


def narrow_latin(pdf):
    import re
    body = pdf[:pdf.index(b"xref\n0 ")].replace(b"/DW 1000 >>", b"/DW 1000 /W [ 1 95 500 ] >>")
    offs = {int(m.group(1)): m.start() + 1 for m in re.finditer(rb"\n(\d+) 0 obj\n", body)}
    out = bytearray(body)
    x = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(offs) + 1) + b"".join(b"%010d 00000 n \n" % offs[n] for n in sorted(offs))
    out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(offs) + 1, x)
    return bytes(out)


T1 = "Sparse Lexical Mixing for Efficient Passage Retrieval"
T2 = "Label-Efficient Segmentation of Satellite Imagery"
T3 = "基于对比学习的中文医疗问答检索"
R_BERT = "BERT: Pre-training of Deep Bidirectional Transformers for Language Understanding"
R_ATT = "Attention Is All You Need"
R_DPR = "Dense Passage Retrieval for Open-Domain Question Answering"
R_UNET = "U-Net: Convolutional Networks for Biomedical Image Segmentation"
R_RES = "Deep Residual Learning for Image Recognition"
R_SAM = "Segment Anything"

paper(
    "papers/sparse-lexical-mixing.pdf", T1, "Mira Okafor, Daniel Reyes and Hana Suzuki",
    "Proceedings of ACL 2024. doi:10.5555/acl.2024.0117",
    "Passage retrieval systems either match words exactly or compare dense vectors. We propose SpaMix, a sparse lexical mixing method that learns a weight for every term. "
    "We evaluate on the MS MARCO and Natural Questions datasets and report MRR@10 and Recall@100. SpaMix improves Recall@100 over strong baselines.",
    [("1 Introduction", "Dense retrievers such as DPR need large indexes. Our approach keeps an inverted index and only changes the term weights."),
     ("2 Experiments", "We train SpaMix on the MS MARCO training split. Datasets: MS MARCO, Natural Questions.")],
    [f"[1] J. Devlin, M. Chang, K. Lee. {R_BERT}. NAACL, 2019. doi:10.18653/v1/N19-1423",
     f"[2] A. Vaswani, N. Shazeer, N. Parmar. {R_ATT}. NeurIPS, 2017.",
     f"[3] V. Karpukhin, B. Oguz, S. Min. {R_DPR}. EMNLP, 2020. doi:10.18653/v1/2020.emnlp-main.550"],
    gold_items=[("Paper", T1, {"doi": "10.5555/acl.2024.0117", "year": "2024"}), ("Person", "Mira Okafor"), ("Person", "Daniel Reyes"), ("Person", "Hana Suzuki"),
                ("Venue", "ACL"), ("Method", "SpaMix"), ("Dataset", "MS MARCO"), ("Dataset", "Natural Questions"), ("Metric", "MRR"), ("Metric", "Recall"),
                ("Paper", R_BERT, {"cited_by": "papers/sparse-lexical-mixing.pdf"}), ("Paper", R_ATT, {"cited_by": "papers/sparse-lexical-mixing.pdf"}),
                ("Paper", R_DPR, {"cited_by": "papers/sparse-lexical-mixing.pdf"})],
)
paper(
    "papers/label-efficient-segmentation.pdf", T2, "Priya Raman, Tomas Novak",
    "arXiv preprint 2024. arXiv:2403.01882",
    "Pixel labels for satellite imagery are expensive. This paper introduces GeoSeg-Lite, a segmentation model that learns from a few labelled tiles. "
    "We use the SpaceNet 7 dataset and the xView2 benchmark and report mIoU and F1. GeoSeg-Lite reaches the accuracy of a fully supervised U-Net with ten times fewer labels.",
    [("1 Introduction", "Fully supervised segmentation follows U-Net and its successors. We ask how few labels are enough."),
     ("2 Results", "On the xView2 benchmark GeoSeg-Lite improves mIoU by 4 points.")],
    [f"[1] O. Ronneberger, P. Fischer, T. Brox. {R_UNET}. MICCAI, 2015.",
     f"[2] K. He, X. Zhang, S. Ren. {R_RES}. CVPR, 2016. doi:10.1109/CVPR.2016.90",
     f"[3] A. Kirillov, E. Mintun, N. Ravi. {R_SAM}. ICCV, 2023.",
     f"[4] M. Okafor, D. Reyes, H. Suzuki. {T1}. ACL, 2024."],
    gold_items=[("Paper", T2, {"year": "2024", "arxiv": "2403.01882"}), ("Person", "Priya Raman"), ("Person", "Tomas Novak"), ("Venue", "arXiv"),
                ("Method", "GeoSeg-Lite"), ("Dataset", "SpaceNet 7"), ("Dataset", "xView2"), ("Metric", "mIoU"), ("Metric", "F1"), ("Metric", "Accuracy"),
                ("Paper", R_UNET, {"cited_by": "papers/label-efficient-segmentation.pdf"}), ("Paper", R_RES, {"cited_by": "papers/label-efficient-segmentation.pdf"}),
                ("Paper", R_SAM, {"cited_by": "papers/label-efficient-segmentation.pdf"}), ("Paper", T1, {"cited_by": "papers/label-efficient-segmentation.pdf"})],
)
paper(
    "papers/medical-qa-contrastive.pdf", T3, "李明华、周雅婷、陈思远",
    "EMNLP 2023。doi:10.5555/emnlp.2023.0452",
    "医疗问答检索需要同时理解口语化的问题和专业的答案。我们提出了MedCL方法，用对比学习把问题和答案拉到同一个向量空间。"
    "实验在CMedQA数据集和webMedQA数据集上进行，指标为F1和MRR，MedCL明显优于基线。",
    [("1 引言", "已有工作如稠密检索依赖大量标注。本文使用对比学习减少标注。"),
     ("2 实验", "我们在CMedQA数据集上训练，并在webMedQA数据集上评估。")],
    [f"[1] V. Karpukhin, B. Oguz, S. Min. {R_DPR}. EMNLP, 2020.",
     f"[2] M. Okafor, D. Reyes, H. Suzuki. {T1}. ACL, 2024.",
     f"[3] J. Devlin, M. Chang, K. Lee. {R_BERT}. NAACL, 2019."],
    zh=True,
    gold_items=[("Paper", T3, {"year": "2023", "doi": "10.5555/emnlp.2023.0452"}), ("Person", "李明华"), ("Person", "周雅婷"), ("Person", "陈思远"), ("Venue", "EMNLP"),
                ("Method", "MedCL"), ("Dataset", "CMedQA"), ("Dataset", "webMedQA"), ("Metric", "F1"), ("Metric", "MRR"),
                ("Paper", R_DPR, {"cited_by": "papers/medical-qa-contrastive.pdf"}), ("Paper", T1, {"cited_by": "papers/medical-qa-contrastive.pdf"}),
                ("Paper", R_BERT, {"cited_by": "papers/medical-qa-contrastive.pdf"})],
)

# ------------------------------------------------------------------ questions (frozen)
QUESTIONS = [
    ("q01", "instruction", True, "What does AGENTS.md say about running tests?", "Run the unit tests with cargo test -p harborq-core before every commit; the web code must pass pnpm --dir web test", ["harborq/AGENTS.md"]),
    ("q02", "instruction", True, "Which files must not be edited by hand according to AGENTS.md, and what should be run instead?", "files under core/src/generated/; run make generate", ["harborq/AGENTS.md"]),
    ("q03", "instruction", True, "Which command formats the Rust code?", "cargo fmt --all", ["harborq/AGENTS.md"]),
    ("q04", "instruction", True, "What retry policy did the second ADR choose, and how many times is a job retried at most?", "exponential backoff with jitter; 5 times", ["harborq/docs/adr/0002-retry-with-backoff.md"]),
    ("q05", "instruction", True, "Why does Harbor Queue use SQLite for storage?", "so that deployment is a single file; small services cannot run a separate database", ["harborq/docs/adr/0001-use-sqlite-for-storage.md"]),
    ("q06", "code", True, "Which function in core/src/retry.rs computes the delay before a retry?", "next_delay", ["harborq/core/src/retry.rs"]),
    ("q07", "code", True, "Which Python packages does tools/export.py import?", "click; requests; rich", ["harborq/tools/export.py"]),
    ("q08", "paper", True, "Which datasets does the SpaMix paper evaluate on?", "MS MARCO; Natural Questions", ["papers/sparse-lexical-mixing.pdf"]),
    ("q09", "paper", True, "Who wrote the paper about label-efficient segmentation of satellite imagery?", "Priya Raman; Tomas Novak", ["papers/label-efficient-segmentation.pdf"]),
    ("q10", "unanswerable", False, "What does AGENTS.md say about deploying to production?", "", []),
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(HERE.parent.parent.parent / "testing" / "super-ontology" / "profiles"))
    ap.add_argument("--check", action="store_true")
    a = ap.parse_args()
    out = Path(a.out)
    gold_files = {}
    for k, rows in GOLD.items():
        gold_files[f"gold/{k}.jsonl"] = "".join(json.dumps(r, ensure_ascii=False, sort_keys=True) + "\n" for r in sorted(rows, key=lambda r: (r["file"], r["label"], r["name"])))
    gold_files["gold/questions.jsonl"] = "".join(json.dumps({"id": i, "kind": k, "answerable": ans, "question": q, "answer": g, "files": f, "lang": "en"}, ensure_ascii=False, sort_keys=True) + "\n" for i, k, ans, q, g, f in QUESTIONS)
    tree = {f"project/{p}": d for p, d in FILES.items()}
    tree.update({p: d.encode() for p, d in gold_files.items()})
    readme = (HERE.parent.parent.parent / "testing" / "super-ontology" / "profiles" / "README.md")
    manifest = "".join(f"{hashlib.sha256(d).hexdigest()}  {p}\n" for p, d in sorted(tree.items()))
    if a.check:
        bad = [p for p, d in tree.items() if not (out / p).exists() or (out / p).read_bytes() != d]
        if (out / "MANIFEST.sha256").read_text() != manifest:
            bad.append("MANIFEST.sha256")
        print("check:", "0 differences" if not bad else f"{len(bad)} differ: {bad[:5]}")
        sys.exit(1 if bad else 0)
    for p, d in tree.items():
        t = out / p
        t.parent.mkdir(parents=True, exist_ok=True)
        t.write_bytes(d)
    (out / "MANIFEST.sha256").write_text(manifest)
    print(f"wrote {len(FILES)} files, {sum(len(v) for v in GOLD.values())} gold items, {len(QUESTIONS)} questions")


if __name__ == "__main__":
    main()
