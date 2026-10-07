# file-import-pg handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a308`. Owner a308. Fixed archive `72dc9e2db841de4de368216bbbe13d738f850ea9`. Passing proof container `c73cdfd60ea2d31bfbeda184754a47eade2207df2fb4a8fec9479215285d1b14`. Repo head recorded by that launch: `e6ebf93b5ac261c097a24d145611e0300e4f88b5`.

## Owned paths

- `tools/file-import-pg/**` including `Cargo.lock` copied by `linux/proof/launch.sh` after docker exit 0.
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a308/SP05/file-import-pg/**`

No root `Cargo.toml` or `Cargo.lock` edit. No other product edit. The tool is a standalone crate and is not a root workspace member.

## Proof map

- `linux/smoke/` pre-overlay smoke. `smoke.exit` 0. `phases/fetch-offline.exit` 101. Left as-is.
- `linux/proof-failed-1` through `linux/proof-failed-6/` preserved failed proof logs, source blobs, and `ATTEMPT.txt`.
- `linux/proof/` passing run. Phase exits are all 0. `docker-run.exit` is 0. `live.stdout` is one passed test in 4.66s. `refuse.stdout` is 6 passed tests.
- `pg/up.sh` and `pg/pg-wrap.sh` are the reviewed recipes. PostgreSQL was already up for the passing run and was not started again.
- `linux/proof/recipe-origin.txt` lists origin path and SHA-256.
- `REPORT.md` and `FINDINGS.md` are the result narrative.
- `cleanup.txt` is the own-resource removal log.
- `SHA256SUMS` and `product-SHA256SUMS` are written by `tools/sha256.rs` after this handoff. `linux/proof/source-sha256.txt` is coreutils `sha256sum` from inside the proof and is not the sealed inventory.

## What the next reader can rely on

The passing test imported CSV, TSV, and JSONL through `oo-file-parser-worker`, projected canonical batches, and COPY'd them into a LOGGED shadow on PostgreSQL 17.10. Replay, hash conflict, hash drift before the child, malformed width, truncated JSONL, overflow collapsed to `malformed`/`protocol`, missing field, token-null distinctions, public SELECT denial (`42501`), and a `pg_terminate_backend` retry are in that one test. Details and the worker token collapse are in `FINDINGS.md`.

Unsupported here: append, upsert, Parquet, Excel, sqlite, dumps, production-untrusted parsing, and runsc. Successful import stays `awaiting_review`. Publication is separate.

## Cleanup

`cleanup.txt` records, in order: `docker stop` and `docker rm` of `oo-a308-pg`; `docker volume rm` of `oo-a308-eef831aa-cargo`, `oo-a308-eef831aa-target`, and `oo-a308-eef831aa-pgdata`; `docker network rm oo-a308-eef831aa-net`; removal of `/tmp/oo-a308-eef831aa-dba4-4af2-a78b-541bc9e0a308`. The network had only `oo-a308-pg`. No prune. No foreign container, volume, or network was removed. After that log, `docker ps`, `docker volume ls`, and `docker network ls` filtered for this owner and name prefix were empty.

Manifest container `oo-a308-hash` ran after this file, on the pinned Rust image, with `--network none`, `--cap-drop ALL`, 6GiB, and 1 CPU. It compiled `tools/sha256.rs` and wrote the sums. The container exited 0. The host wrapper then failed on a read-only zsh variable before `docker rm`, and a following `docker rm oo-a308-hash` removed that exited container. The sums were rewritten once after that sentence, so this file is inside `SHA256SUMS`. `product-SHA256SUMS.sha256` uses the tool's fixed stamp label `SHA256SUMS`.
