# Dependency proposal for SP09-W05

Status: proposal only. `audited` is false. This file does not edit `supply-chain/dependency-baseline.json`, root `Cargo.toml`, or root `Cargo.lock`. It does not admit a crate, a binary, a font, or an image.

Authority for the shape: `supply-chain/policy.md` sections 1, 2, 4, and 9, and `.github/ISSUE_TEMPLATE/dependency.yml`. The machine-readable copy is `dependency-proposal.json`. Empty required fields mean the row is not admissible. Nobody is appointed while `OWNER_PENDING` remains, so a seven-day security exception cannot be used.

The card completion definition stays Plan SP09-W05: authorized Parquet, local DuckDB and Arrow, Chinese charts, formula-safe CSV, Excel, source manifest, native resource limits, and a killable native exit. This proposal is what is missing. It is not permission to call CSV and SVG a completed card.

## Why these rows are blocked

`supply-chain/dependency-baseline.md` records candidate compiles. A compile is not admission. The workspace lock still has no `duckdb`, `arrow`, `parquet`, or `calamine` package. The same file says `oo-analysis` cannot depend on the rust `duckdb` crate, and that a later sandbox DuckDB CLI is not a crate dependency. `docs/17-rust-implementation.md` says Arrow and Parquet stay on one series, and DuckDB's own Arrow series crosses that boundary only as Arrow IPC. It also says Excel export uses a reviewed Rust writer or an established CLI, and that a CJK font is fixed in the tool image rather than downloaded at runtime.

No digest, LICENSE file, NOTICE file, advisory scan, or RFC3339 publication time was re-established by this card. Those fields stay null. Null is not a pass.

## Row A — DuckDB CLI, not duckdb-rs

| Field | Value |
| --- | --- |
| Ecosystem | native or image |
| Name | DuckDB CLI, engine patch recorded separately from any crate version |
| Version | Not selected. Baseline compiled crate `duckdb` 1.10505.0 feature `bundled` and refused 1.10506.0 for age. That crate must not be added to `oo-analysis` or to `tools/local-analysis`. |
| Source | Not copied here. Official CLI build would have to name the upstream repository and the exact engine patch. |
| Manifest | None. A future manifest would be an image digest or a pinned binary path, not a new root Cargo dependency, until reviewers say otherwise. |
| Publication and adoption | Not established as RFC3339. Seven-day rule is not met by this proposal. |
| MSRV | Not a Rust crate if the CLI is the artifact. The forbidden `duckdb` crate path is not proposed. |
| Features | CLI must disable extension auto-install and auto-load, reject remote URIs, and read only the authorized input directory. `default_features_disabled` is not applicable to a CLI and is stated as `not a Cargo crate`. |
| License | Baseline names MIT for DuckDB and duckdb-rs. This proposal does not attach a LICENSE file, so the SPDX field is null. |
| Native transitive | The bundled engine is C++. Its LICENSE and NOTICE are not in this proposal. Unknown native licenses mean the row is unfinished. |
| Digest | None. A floating tag is not a digest. |
| Advisory | `scan_recorded` false. No `cargo audit`, `cargo deny`, or native scanner was run. |
| Seven-day exception | Not requested. `OWNER_PENDING` would make it invalid anyway. |

Intended use after admission, not in this card: spawn that fixed CLI as a child with a private temp directory, no network, no extension install, CPU and memory limits, and kill the child on cancel. Arrow IPC is the boundary if the CLI's Arrow series is not series 60. Do not cast DuckDB Arrow memory into the Rust Arrow 60 types.

## Row B — Arrow and Parquet series 60

| Field | Value |
| --- | --- |
| Ecosystem | cargo |
| Name | `arrow` and `parquet`, same series |
| Version | Baseline candidate 60.0.0. Not in the workspace lock. |
| Source | crates.io, repository not re-read for this proposal |
| Manifest | Would be a reviewed manifest. This card must not edit the root lock. |
| Features | Not selected. Default features are not a security configuration and are not declared off, because the row is not filled. |
| Series rule | Do not force series 60 into the same resolve as `duckdb` 1.10505.0, which resolved Arrow 58.4.0 in the baseline probe. |
| License, digest, advisory, publication | Null. Not admissible. |

## Row C — Excel writer

`calamine` 0.36.1 is a baseline read candidate. It does not write xlsx. This proposal does not name an unreviewed writer crate as the selection. A writer still needs every policy section 1 field, including native ZIP transitive licenses if the crate vendors them. Until that record is audited, `local-analysis` exits `xlsx` and writes no workbook. Formula safety for Excel remains an unmet AT-API-04 check. The CSV apostrophe rule is not an Excel implementation.

## Row D — CJK font

The chart text is UTF-8 because the SVG is generated in Rust. No font file is shipped. A future image row would name Noto CJK or Source Han Sans with an immutable digest, a LICENSE file, and an offline package list. Publication time is not established here. The SVG `font-family` string is not that package.

## What a reviewer would still have to do

Fill every null field from the package's own LICENSE, NOTICE, checksum, and an advisory scan. Set `audited` true only after that evidence exists. Appointed owners are required for any seven-day exception. This card does not set `audited` and does not assign reviewers.

## What this card built instead

`tools/local-analysis` has no registry dependency. Its lock is the package itself. That is independent work while these rows are open. The manifest field `closes_sp09_w05` is false.
