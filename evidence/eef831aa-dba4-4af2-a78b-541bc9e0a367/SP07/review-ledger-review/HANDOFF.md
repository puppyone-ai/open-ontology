# HANDOFF

Verdict: **APPROVE_LIMITED** for original E52-D1/1 at `6b3a92c2e06a2e91a74393b739ec8f1c565ae17b`.

Session: `eef831aa-dba4-4af2-a78b-541bc9e0a367`.

Owned evidence path: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a367/SP07/review-ledger-review/**` only. Product trees and the repository root were read-only. No acceptance count, plan checkbox, or ledger line was changed.

## What a later reader can treat as accepted

The durable private-Postgres queue, one-transaction decision plus audit, revision, scope, stale hash, idempotency, auditor batch refusal before any row or tuple change, and one real description-pack receipt that stays unpublished, unapplied, and ungranted. The malformed pack command writes no receipt. A declared operator file is not a verified management identity and is not a publish claim.

## What stays open

Public RBAC, authenticated HTTP, Git curated apply, full S9 and S10, SP07-W05, and the parent goal. See `FINDINGS.md` for the receipt transaction, the reviewer-principal gap, drift files, the runtime `UPDATE` grant, `commit.unknown`, and the toolchain pin.

## How this packet was produced

`linux/smoke-before.sh` ran before the oracle existed and before the author rerun. `linux/proof.sh` commands used here were the resumed author phases and a later `oracle` phase. Recipes and their origin hashes are `origin/recipes.txt`. The pg-wrap bytes are the a274 file, SHA-256 `d53f478805b2e721c81d5d8befafe70d4e6bac71af6136d2531149f70afe2bbb`.

Author phases are not idempotent on a populated `oo_a358_review`. The oracle is not idempotent on a populated `oo_a367_oracle`. `restart` only compares `linux/author/history.sha256`. A new write proof needs an empty database. Do not point those tests at a shared or customer instance.

`linux/proof.exit` is the last driver status, `0`, from the oracle rerun. The earlier failure is `linux/proof-driver.log` plus `failed-attempts/`. `failed-attempts/oracle.log` duplicates `failed-attempts/attempt-2-oracle-clippy.log`.

Secrets, while the instance existed, were `/tmp/oo-a367-eef831aa-dba4-4af2-a78b-541bc9e0a367/secrets` mode `700`. The repository copy is `pg/ca.sha256` and `pg/server-san.txt`. Owned cleanup is `CLEANUP.md`.

## Next integration

Management RBAC, an HTTP control API, and Git apply of curated files are the next surfaces. They are outside this crate. `commit.unknown` stays a non-retried ambiguous error. Do not hide a lost commit reply and do not retry it blindly.

The description-pack combined reseal that landed after this commit needs its own review. It is not covered by the receipt hashes in `oracle/summary.json`.
