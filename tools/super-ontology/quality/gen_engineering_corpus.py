#!/usr/bin/env python3
"""Synthetic engineering repository for the entity quality gate (D90).

Writes <out>/docs/... (design notes, ADRs, a changelog, runbooks, READMEs and a little
code) and <out>/truth.json: the decisions the corpus really states (actor + action +
object, each in one sentence) and the generic words it is full of ("rollback",
"prepare", "consistency") that must never become a Decision, Event or Person.
All names are made up. Deterministic: no clock, no randomness.

Usage: gen_engineering_corpus.py OUT_DIR
"""
import json
import sys
from pathlib import Path

TRUE_DECISIONS = [
    ("Mara Lindqvist", "adopt two-phase commit for the ledger service", "docs/design/ledger-commit.md"),
    ("Tomas Reyes", "move the job queue from polling to LISTEN/NOTIFY", "docs/design/job-queue.md"),
    ("Priya Nair", "keep the snapshot format at version 3 until the v4 reader ships", "docs/design/snapshot-format.md"),
    ("Mara Lindqvist", "reject the shared-nothing sharding proposal", "docs/design/sharding.md"),
    ("Jonas Weber", "retire the legacy importer in the 2.4 release", "docs/adr/0007-retire-importer.md"),
    ("林远舟", "统一用单写者模式处理账本写入", "docs/design/账本写入.md"),
    ("周若溪", "回滚脚本必须在发布前演练一次", "docs/runbook/发布与回滚.md"),
    ("Tomas Reyes", "use LISTEN/NOTIFY for the job queue", "Decisions/0010-use-listen-notify.md"),
]

FILES = {}

def put(path, text):
    FILES[path] = text.strip() + "\n"

put("README.md", """
# Quill

Quill is a small embedded ledger database. This repository holds the engine, the CLI and the design notes.

## Layout

- `src/` the engine
- `docs/design/` design notes
- `docs/adr/` architecture decision records
- `docs/runbook/` operations

Rollback, prepare and consistency are the three words every reviewer asks about; see the ledger design note.
""")

put("CHANGELOG.md", """
# Changelog

## 2.4.0

- Added: prepare phase timeout is now configurable.
- Changed: rollback of a half-written segment no longer scans the whole log.
- Fixed: consistency check reported false positives on empty segments.
- Removed: the legacy importer.

## 2.3.2

- Fixed: rollback after a crash during compaction.
- Fixed: prepare returned before the fsync finished.

## 2.3.0

- Added: snapshot format version 3.
- Added: consistency mode `strict`.
- Changed: retry policy for the job queue.
""")

put("docs/design/ledger-commit.md", """
# Ledger commit protocol

Owner: Mara Lindqvist

## Context

The ledger writes to two segments. A crash between the writes leaves them inconsistent.

## Decision

Mara Lindqvist decided to adopt two-phase commit for the ledger service, on 2026-03-02.

## Protocol

1. Prepare: every segment writes its intent record and fsyncs.
2. Commit: the coordinator writes the commit record.
3. Rollback: on any prepare failure the coordinator writes an abort record.

## Terms

- **Rollback**: undo of a prepared but uncommitted write.
- **Prepare**: first phase of the commit.
- **Consistency**: the invariant that both segments agree after recovery.

## Alternatives rejected

- Single segment with a write-ahead log
- Saga style compensation
""")

put("docs/design/job-queue.md", """
# Job queue

Owner: Tomas Reyes

The queue polled the table every second. Tomas Reyes decided to move the job queue from polling to LISTEN/NOTIFY, 2026-04-11.

## Decisions

- Rollback
- Prepare
- Consistency
- Tomas Reyes decided to move the job queue from polling to LISTEN/NOTIFY.

## Notes

Prepare statements are cached per connection. Rollback of a failed job releases its lock.
""")

put("docs/design/snapshot-format.md", """
---
owner: Priya Nair
---

# Snapshot format

Priya Nair decided to keep the snapshot format at version 3 until the v4 reader ships.

The v4 format adds a consistency footer. Readers of v3 ignore it.

## Open questions

- Consistency of the footer under concurrent compaction
- Prepare hooks for incremental snapshots
""")

put("docs/design/sharding.md", """
# Sharding

The team discussed sharding twice. Mara Lindqvist rejected the shared-nothing sharding proposal on 2026-05-20 because rebalancing needs a consistency story that does not exist yet.

## Terms

Consistency, rollback and prepare are used as in the ledger note.

## Tried

- Hash partitioning by account
- Range partitioning by date
""")

put("docs/adr/0007-retire-importer.md", """
# ADR-0007 Retire the legacy importer

Status: accepted
Date: 2026-06-01
Owner: Jonas Weber

Jonas Weber decided to retire the legacy importer in the 2.4 release. Customers migrate with `quill load`.

## Consequences

Rollback of an import is no longer needed. Consistency checks remain.
""")

put("docs/adr/0008-error-codes.md", """
# ADR-0008 Error codes

Status: proposed

## Decision

Errors carry a stable numeric code.

## Context

Support tickets quote messages, which change between releases.
""")

put("docs/design/账本写入.md", """
# 账本写入

负责人：林远舟

## 决定

- 林远舟决定统一用单写者模式处理账本写入。
- 唯一改写
- 一致性
- 回滚
- 准备

## 说明

唯一改写入口是 `LedgerWriter`。一致性检查在恢复后运行。回滚只发生在准备阶段失败时。
""")

put("docs/runbook/发布与回滚.md", """
# 发布与回滚

负责人：周若溪

## 流程

周若溪决定回滚脚本必须在发布前演练一次。

- 准备：冻结写入
- 发布：滚动升级
- 回滚：恢复上一个快照
- 一致性：运行校验

## 术语

**一致性**：恢复后各段数据一致。
**回滚**：撤销未提交的写入。
""")

put("docs/runbook/oncall.md", """
# On-call runbook

Owner: Jonas Weber

## Rollback

If the release fails the smoke test, roll back with `quill release rollback`.

## Prepare

Run `quill release prepare` the day before.

## Consistency

After any rollback run `quill check --consistency`.
""")

for i, (topic, line) in enumerate([
    ("compaction", "Compaction merges small segments. The consistency check runs after each merge. A failed merge triggers rollback of the new segment."),
    ("recovery", "Recovery replays the log. Prepare records without a commit are rolled back. Consistency is verified at the end."),
    ("backup", "Backups copy sealed segments. Prepare the backup window with `quill backup prepare`. Rollback of a restore needs the previous snapshot."),
    ("replication", "Followers apply the log in order. Consistency lag is exported as a metric. A follower that cannot prepare reports failure."),
    ("cli", "The CLI wraps the engine. `quill rollback` is the only command that rewrites history. `quill prepare` is read only."),
    ("metrics", "Metrics include prepare latency, commit latency and rollback count. Consistency violations are counted per segment."),
    ("locking", "Locks are taken in prepare and released in commit or rollback. Consistency of the lock table is checked on startup."),
    ("config", "Settings: `prepare_timeout_ms`, `rollback_batch`, `consistency_mode`. Defaults are listed in the table below.\n\n| key | default |\n|---|---|\n| prepare_timeout_ms | 5000 |\n| rollback_batch | 128 |\n| consistency_mode | strict |"),
]):
    put(f"docs/design/{topic}.md", f"# {topic.capitalize()}\n\n{line}\n\n## Notes\n\nSee also the ledger commit note. Rollback, prepare and consistency are defined there.\n")

for i in range(1, 9):
    put(f"docs/meetings/2026-0{(i % 9) + 1}-design-sync.md", f"""
# Design sync {i}

Attendees: Mara Lindqvist, Tomas Reyes, Priya Nair

- Reviewed rollback timings for segment {i}.
- Prepare latency is within budget.
- Consistency check is flaky on CI; Priya Nair will investigate.
- Action: Tomas Reyes owns the fix for the flaky consistency check.
""")

for i in range(1, 7):
    put(f"docs/notes/note-{i}.md", f"""
# Note {i}

Consistency, rollback and prepare again. Quick thoughts on the commit path.

The word rollback appears in the log {i} times; prepare {i + 1} times. Nothing was decided.
""")

put("docs/glossary.md", """
# Glossary

- **Rollback**: undo of a prepared write.
- **Prepare**: first phase of two-phase commit.
- **Consistency**: agreement of segments after recovery.
- **Segment**: a log file.
""")

put("src/ledger.rs", """
//! Ledger. Two-phase commit: prepare, commit, rollback.
pub fn prepare() {}
pub fn commit() {}
pub fn rollback() {}
pub fn check_consistency() {}
""")
put("src/queue.py", """
# job queue: LISTEN/NOTIFY
def prepare(): pass
def rollback(): pass
""")
put("src/README.md", "# src\n\nEngine sources. Rollback and prepare live in `ledger.rs`.\n")


# Decision records by directory convention: one is a real decision, two are a bare word with notes (false).
put("Decisions/0010-use-listen-notify.md", """
---
owner: Tomas Reyes
date: 2026-04-11
---

# Decision: use LISTEN/NOTIFY for the job queue

Tomas Reyes decided on 2026-04-11 to use LISTEN/NOTIFY for the job queue instead of polling the table every second.
""")
put("Decisions/rollback.md", """
# Rollback

Notes on rollback.
""")
put("Decisions/准备.md", """
# 准备

准备阶段的笔记。
""")

# Files that exist twice (same bytes) and renamed, for the review measurements.
put("docs/design/ledger-commit-copy.md", FILES["docs/design/ledger-commit.md"])
put("archive/ledger-commit.md", FILES["docs/design/ledger-commit.md"])
put("archive/job-queue-v2.md", FILES["docs/design/job-queue.md"])
put("archive/snapshot-format.md", FILES["docs/design/snapshot-format.md"])

# Chinese notes with generic words
for i, w in enumerate(["回滚", "准备", "一致性", "唯一改写", "幂等", "重试"]):
    put(f"docs/notes/术语-{w}.md", f"# {w}\n\n这里记录{w}的用法。{w}出现在提交路径里，没有做出任何决定。\n")

def main():
    out = Path(sys.argv[1])
    for rel, text in FILES.items():
        p = out / "docs" / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text, encoding="utf-8")
    truth = {
        "decisions": [{"actor": a, "action": t, "file": f} for a, t, f in TRUE_DECISIONS],
        "generic_words": ["rollback", "prepare", "consistency", "回滚", "准备", "一致性", "唯一改写", "幂等", "重试", "segment", "compaction"],
        "files": len(FILES),
    }
    (out / "truth.json").write_text(json.dumps(truth, ensure_ascii=False, indent=1), encoding="utf-8")
    print(f"{len(FILES)} files under {out}/docs")

main()
