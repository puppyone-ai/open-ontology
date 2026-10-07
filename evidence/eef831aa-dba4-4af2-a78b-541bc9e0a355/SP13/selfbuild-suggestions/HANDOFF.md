# Handoff

Owner session `eef831aa-dba4-4af2-a78b-541bc9e0a355`. Fixed base for the query-pg archive: `7a6f7eac8b8041eae215c087d1a80930437871ce`.

## Owned paths

- `tools/selfbuild-suggestions/**` — contract, crate, focused tests.
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a355/SP13/selfbuild-suggestions/**` — this evidence only.

Commit those paths alone. Do not stage the rest of the dirty worktree. Do not push, open a pull request, or accept the parent plan or the SP13 goal.

## What is done

The crate emits deterministic S6 entity and domain candidates, S7 metric hypotheses, S8 question and SQL templates, and an S9 queue. S8 calls the compiled `query-pg` only when an operator config is supplied. Without a config, examples stay `not_run`. `knowledge_verified` is false. Feedback is untrusted. `auto_merge`, grants, certification, curated writes, and publish stay false.

Read `REPORT.md` for the behavior, `FINDINGS.md` for failed attempts, and `live/record.txt` for the gateway result. Raw exits:

- `build/test.exit` 0, `build/clippy.exit` 0
- `querypg/build.exit` 0, `querypg/fetch.exit` 0
- `pg/up.exit` 0, `pg/apply.exit` 0
- `live/test.exit` 0

Live result, already finished before this write, not re-run: `gateway_probe_is_refused_before_one_admitted_select` ok in 0.20s. Probe `UPDATE shop.account SET label = label` refused (exit 6, empty stdout). One admitted statement: `SELECT party_key FROM shop.account WHERE party_key IS NOT NULL LIMIT 1`. Second example `not_run`. `gateway.admitted` 1. `knowledge_verified` false.

## What an independent review can check

- Contract versus `src/produce.rs`, `src/gateway.rs`, `src/json.rs`, and `tests/producer.rs`.
- `live/test.txt` and `live/record.txt` against `run_gateway` and `classify`.
- `querypg/binary-sha256.txt` and `querypg/base.txt`.
- `SHA256SUMS` for the owned tree.
- No password or private key in this directory.
- Root `Cargo.toml` and `Cargo.lock` are untouched by this slice.

No further builder or Postgres was started to collect the live result. No new review count belongs to this handoff. Independent review sits outside the 20-development-Grok cap. Frontend work is not part of this crate.

## Runtime leftovers

Owned cleanup after the record was saved removes the exited builder `oo-a355-sbs-live` and stops `oo-a355-pg`. See `CLEANUP.md` for the commands and what remains (private volumes and the bridge network). Secrets were only under `/tmp/oo-a355-sbs-secrets` and are removed there. They are not in git.

The query-pg binary remains on docker volume `oo-a355-sbs-gw-target` at `/target/debug/query-pg` and is not a committed artifact. Rebuilding it means one arm64 Rust 1.94 builder, 6GiB, 1 CPU, `--pull never`, the pinned rust image, and an archive of the fixed base that includes `tools/gateway-token-store`.
