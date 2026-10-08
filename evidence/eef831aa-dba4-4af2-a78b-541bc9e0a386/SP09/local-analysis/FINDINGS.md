# Findings

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a386`.

## Card status

SP09-W05 stays open. The implementation refuses `engine=duckdb`, `xlsx=true`, and `parquet=true` before it reads an export, and it writes nothing for those requests. The manifest fields `duckdb`, `arrow`, `parquet`, `xlsx`, and `cjk_font` are `not_admitted`. `closes_sp09_w05` is hard-coded false. A native child was never spawned, so a signal kill of DuckDB was not demonstrated. Cooperative cancel is the cancel proof: a present cancel file returns exit 1 and creates no output. The unit test `cancel_after_partials_removes_them_before_rename` shows this process deleting only the partials it created. That is not a power-failure claim and not a kill of another process.

`DEPENDENCY-PROPOSAL.md` and `dependency-proposal.json` name DuckDB CLI, Arrow 60, Parquet 60, an unidentified Excel writer, and a CJK font. Required audit fields are null. `audited` is false. Reviewers are unassigned. `OWNER_PENDING` means a seven-day security exception was not requested. The proposal does not edit `supply-chain/dependency-baseline.json`, root `Cargo.toml`, or root `Cargo.lock`. `crates/oo-analysis` was not edited. No xlsx bytes and no DuckDB result were fabricated.

## What the private chain showed

The live numeric proof and the frozen exporter produced this card's export. The analysis binary then read `out/complete` and `audits/amounts.jsonl` from that same run. The fixture under `tools/local-analysis/tests/fixtures` is a copy of the a344 review bytes for unit oracles. Its stdout digest differs from this run, so it was not used as the chain input.

Stdout changed while the exported CSV digest stayed `f5b287fff0d197d55a1d67893178ff7e68ed5a1338830d24ea8fe0f11db2ca3d`. Original SQL SHA-256 stayed `466f22e579afe5252e78ae6cffca1da0a9c4630ba8cb63440a0fc9b7a9c6957a`. Executed SHA-256 for this backend is `034855baad1325ac740d6706f83a42f02549f26096fb37a14aabe0af328f0d9a`. Fingerprint `49b26b99a4752eca` makes query id `q_49b26b99a4752eca`. The a344 receipt fingerprint was `40326e01dcdd4ae7`. Lineage was built from this audit only. `local-analysis` recomputed the receipt SHA-256 and accepted the bind. Trust remains `local_operator_declared`.

Exact decimal behavior on this export: scale 4 is preserved, group 5 renders `0.0000` with coefficient `0` and `negative` false, and the 38-digit wide values survive as text. Formula prefixing did not attach an apostrophe to a typed minus. The null amount on entry 6 increments `null_values` and emits no group. The truncated export is written and marked `不可引用`.

The SVG names `Noto Sans CJK SC` and `Source Han Sans SC` and bundles no font file. Chinese title bytes are literal UTF-8. A reviewer can open `linux/chain/out/analysis/analysis.svg` and `linux/chain/out/analysis-wide/analysis.svg`.

## Checks that did not pass, and why that is recorded

Clippy did not run. The image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5` has rustc 1.94.0 and no installed Clippy component. `linux/test/clippy.exit` is 1. Fetching the component was refused. The test process itself exited 0.

## Boundaries held

Write paths are `tools/local-analysis/**` and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a386/**`. No root manifest change, no `cargo update`, no `cargo generate`, no host `cargo`, no image pull, no push, no release, and no customer call. The chain used one builder at 6 GiB and 1 CPU. PostgreSQL used 1 GiB and 1 CPU, with no host port. Network was the private bridge `oo-a386-eef831aa-net`.
