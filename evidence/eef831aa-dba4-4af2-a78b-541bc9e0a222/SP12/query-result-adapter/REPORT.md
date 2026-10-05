# Query-result adapter

Session `eef831aa-dba4-4af2-a78b-541bc9e0a222`. Product commit `ed661809cf6c1e8df37af5842838dd1eb839c98a`, parent `c5f4019574541296e15628325738864d97e4a47f`. Fixed archive `c2db9c430b822a824a1062b6c3c7345924df7dd8`, tree `8fb187147b02d6cacf45cb5a683859f698b497cf`, saved before the pre-edit smoke. Repository HEAD at the start of this session was `a863ab06796d6169b455fa15f805a2b21c9a0d2c`.

The owned product path is `benchmarks/query-result-adapter/**`. It is a standalone workspace. The root `Cargo.toml` and root `Cargo.lock` were not edited. The result comparator, gateway, runtime, and other product crates were not edited. This batch had no dependency writer. `cargo update` was not invoked (`linux/proof/cargo-update-invoked.txt` is `no`). Lock drift against the root registry pins is empty.

This directory does not accept B-D02, a parent item, a question bank, or a release. The proof is one private PostgreSQL database, one compiled `query-pg`, the compiled adapter, and the compiled comparator.

## What the adapter does

`query-result-adapter` reads one caller header line and one `query-pg` JSONL stdout. The header carries the question-column names, types, ordinals, turn, and budgets. `query-pg` itself has no header record. The output is only the typed actual `oo.benchmark.result-comparison/v1` document the existing comparator already accepts.

Cell kinds from `query-pg` stay `null`, `bool`, `int`, and `text`. An int is a canonical `i64` string, including the minimum int. A decimal column is a binding type. An int cell may fill it only when the text is already a canonical integer. A text cell may fill it only as a decimal literal with no sign on zero, no exponent, and no whitespace. The gateway did not grow a decimal kind. Null, missing (`ordinal` JSON null), and a type disagreement stay distinct. Warnings are an empty array. `partial` and `failed` stay those words. `succeeded` with `end_audit_ok` true is completion `complete`, including a truncated read. `succeeded` with `end_audit_ok` false refuses completion and does not rewrite the status to `failed`. End of file is not success. The stream needs the caller header, one matching terminal summary, row-count agreement before the budget check, and budgets that cover the summary. A duplicate, a trailer, an unknown record, or a missing final newline refuses. Gold is not an adapter input.

## Host and images

Linux arm64 Rust 1.94.0 (`4a4ef493e 2026-03-02`), image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL 17.10 image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Container `oo-a222-pg` id `3682794d143cdff2be5188a60f4ccead580ae2d258d96c7840d2b0d63ef577a0`, aliases `oo-a222-pg` and `oo-a198-pg`, no host port. `pg/up.exit` is 0. Privileged is false.

Each rust container had 6 GiB, 1 CPU, all capabilities dropped, and bridge network `oo-a222-eef831aa-net`. Postgres had 1 GiB. One rust builder at a time. One postgres. Private cargo, target, CA, and password files lived under `/tmp/oo-a222-eef831aa-dba4-4af2-a78b-541bc9e0a222`. The builder mounted the fixture certificates and passwords read-only. Evidence secret scan of the live files is `passwords=clean`, `private_keys=clean`, `banned_values=clean`.

Recipe origins and the later one-line script edits are `ORIGIN.md`.

## Commands and exits

Pre-edit smoke, container `oo-a222-smoke` id `c7b75f63a21721cea7a6caed0302d594aad3bda38750c4282832943dd6245447`, docker exit 0, `OOMKilled=false`, memory 6442450944, NanoCpus 1000000000. The fixed archive had no `query-result-adapter` (`adapter-absent.exit` 0, `comparator-present.exit` 0). `linux/smoke/comparator.exit` is 0. Those tests are the archive result-comparator suite.

Overlay attempts 001 through 006 are preserved under `linux/proof/attempts/` with `ATTEMPT.txt`. The green run is `linux/proof/`. Container `oo-a222-proof` id `e4456b2082c84a1cb6f9c35180d167b4e74dd8a7526d85d0212d0f7c34dc2368`, docker exit 0, `OOMKilled=false`, memory 6442450944, NanoCpus 1000000000, CapDrop ALL, privileged false.

| Phase | Exit |
| --- | --- |
| rustc 1.94.0 identity, toolchain file renamed | 0 |
| archive diff before cargo | 0 |
| adapter source hash | 0 |
| `cargo generate-lockfile` then drift | 0, drift file empty |
| `cargo test --locked --offline` adapter | 0 |
| rustfmt `--check` | 0 |
| `cargo clippy -D warnings` | 0 |
| `cargo fetch --locked` workspace | 0 |
| libclang extract and load | 0 |
| comparator `--offline` build | 0 |
| `query_pg_cli` `--no-run` | 0 |
| wrapper install | 0 |
| live `query-pg` against `oo-a222-pg` | 0, 2.21s |
| desk stdout saved | 0 |
| secret scan | 0 |
| proof test with `OO_A222_PROOF=1` | 0, 0.02s |
| product tree and adapter source unchanged | 0 |

Registry pins printed by the green run: serde 1.0.229 `4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba`, serde_json 1.0.151 `c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14`.

The adapter-gate `cargo test` counts are 11 `adapt` tests, 3 CLI tests, 3 SHA-256 self-checks, and the proof test returning because `OO_A222_PROOF` is unset. The live proof is `linux/proof/proof.stdout`.

`tree-diff-after.txt` lists only `benchmarks/query-result-adapter` added under `/src`, plus the renamed `rust-toolchain.toml`. `source/blobs.txt` holds the git blob ids of the 19 committed crate files. `source/blobs/query-result-adapter/` is that tree.

## Measured facts

`linux/proof/live/oracle.txt`: `star_columns=lane,rank,stamp,note,open`, desk rows 3, peek rows 3, count 3, truncate rows 1, truncate bytes 40, hidden/write/sleep exit 6 with audit absent, invalid CA exit 4, row digest unchanged.

Saved stdout under `linux/proof/live/captures/`: two 937-byte desk results (explicit column list and `SELECT *`), two 565-byte peek results, one 434-byte count (`int` value `3`), one 577-byte truncated desk row, one 999-byte two-turn stdout (count turn 0, then peek turn 1). Desk note code points are U+96EA U+306E U+7968 U+0301. The null note stays presence `null`.

`tests/proof.rs` prepends the handwritten header, runs the compiled adapter, and runs the compiled comparator against handwritten gold. On this stdout the test passed for ordered desk, unordered peek, ordered count, truncated desk (completion `complete`, one row, comparator mismatch), the two-turn stream (protocol refusal under the count header), a changed `beta` value (comparator mismatch), `end_audit_ok` false (stderr `query-result-adapter: completion /summary/end_audit_ok`), status `partial` (completion `partial`, comparator mismatch), and a dropped summary (protocol refusal).

## Cleanup

Own containers, volumes, and the private network are removed after these reports. The log is `cleanup/run.log`. Pinned images stay. No prune.
