# Dependency proposal for SP09-W05

Status: proposal with the fields this card could verify. `audited` is false. `reviewers_assigned` is empty. `exception` is null. This file does not edit `supply-chain/dependency-baseline.json`, root `Cargo.toml`, or root `Cargo.lock`. It does not appoint a reviewer and it does not close SP09-W05.

Adoption instant for the age arithmetic below: `2026-10-08T05:14:59Z` (the private builder fetched the CLI and the font). Age days are `floor(elapsed_seconds / 86400)`. Seven days is `604800` seconds. No security-fix exception is requested. `OWNER_PENDING` would make one invalid.

`scan_recorded` is false. No `cargo audit`, `cargo deny`, or native advisory scanner was run. A missing scan is not `no-known-advisories`.

The child crate is `tools/local-analysis`. Its lock is its own. The rust `duckdb` crate is not a dependency.

## Direct crates

| Package | Version | crates.io published | elapsed_s | age_days | MSRV | Features | Digest |
| --- | --- | --- | --- | --- | --- | --- | --- |
| arrow | =60.0.0 | 2026-09-15T15:31:03Z | 1950236 | 22 | 1.88 | default off, `ipc` | 234ce58e76c04e8162c2e4bc3fe53fa7c2418ef5fba7bb2838b8531b7d4fbf14 |
| parquet | =60.0.0 | 2026-09-15T15:32:03Z | 1950176 | 22 | 1.88 | default off, `arrow`, `snap` | 8af83d2940bc0510f9aef86d865f56fdc6095f87ab115ac885a80b7c5226d3ba |
| rust_xlsxwriter | =0.99.1 | 2026-09-14T07:22:55Z | 2065924 | 23 | 1.88.0 | default off, `zlib` not enabled | 34f2d2cda7223168a14fb86bc7411dbc3379bd7d49749ecc53b91ed98585b6b0 |
| zip | =8.3.0 | 2026-03-19T16:19:08Z | 17499351 | 202 | 1.88 | dev-only, default off, `deflate` | 4a243cfad17427fc077f529da5a95abe4e94fd2bfdb601611870a6557cc67657 |

`zip` is a dev-dependency so the formula test can read the xlsx. The same version is what `rust_xlsxwriter` 0.99.1 resolves when the requirement `^8.3` is pinned. Its `deflate` feature selects flate2's `zlib-rs` backend, not C zlib.

License evidence:

- Arrow and Parquet: Apache-2.0. `https://github.com/apache/arrow-rs/blob/60.0.0/LICENSE.txt`. Repository `https://github.com/apache/arrow-rs`. Series 60 is not resolved with the refused rust `duckdb` crate.
- `rust_xlsxwriter`: `MIT OR Apache-2.0`. This project relies on Apache-2.0. Both texts are in the tag: `LICENSE_Apache2.0` and `LICENSE_MIT` at `https://github.com/jmcnamara/rust_xlsxwriter/blob/v0.99.1/`. Obligation kept: retain the copyright and license notices.
- `zip`: MIT. `https://github.com/zip-rs/zip2/blob/v8.3.0/LICENSE`. Obligation kept: retain the copyright notice.

Enabled features are not a security configuration by themselves. Defaults are off so the C compression stacks on Parquet (brotli, zstd, lz4, flate2) and the C `zlib` feature on the Excel crate stay out.

## DuckDB CLI

Not a Cargo crate. Official asset `duckdb_cli-linux-arm64.gz` from tag `v1.5.6`.

| Field | Value |
| --- | --- |
| Archive URL | `https://github.com/duckdb/duckdb/releases/download/v1.5.6/duckdb_cli-linux-arm64.gz` |
| Archive updated | 2026-09-28T13:36:15Z |
| Archive bytes | 19200830 |
| Archive SHA-256 | `1af0541e649a3ae34eb20eec2ca564308bf0e8993e5d6dba6be77bfb289a2718` |
| Unpacked bytes | 54304672 |
| Unpacked SHA-256 | `c0ac0b79e243ee312b3af29c34ac0b94e518aa44c4345036d5d96c1c042c9d48` |
| Version string | `v1.5.6 (Variegata) 069cc9f9b5` |
| License | MIT. `https://github.com/duckdb/duckdb/blob/v1.5.6/LICENSE` |
| Age | elapsed 833924 seconds, 9 days |
| MSRV | not a Rust crate |

The binary is the upstream build. This card does not compile DuckDB's C++ tree. The tool refuses any other unpacked digest. Runtime SQL sets one thread, a 256MB memory limit, disables extension auto-install and auto-load, allowlists only the private temp directory, sets `enable_external_access` false, then locks configuration. A probe on 2026-10-08 showed `enable_external_access` false without the allowlist cannot `read_parquet`, and the same setting with the allowlist can. The child environment is cleared. Cancel kills the child.

## CJK font

Noto Sans CJK SC, tag Sans2.004. Not a Rust crate. Not linked into the binary. The chart copies the verified Regular OTF next to the SVG.

| Field | Value |
| --- | --- |
| Archive URL | `https://github.com/notofonts/noto-cjk/releases/download/Sans2.004/08_NotoSansCJKsc.zip` |
| Archive updated | 2022-03-20T06:35:14Z |
| Archive bytes | 94523633 |
| Archive SHA-256 | `a927e56f53bd6c3b920bc139c0b94aa36c7d9ad0cf009b159437a1a003581140` |
| File | `NotoSansCJKsc-Regular.otf` |
| File bytes | 16437364 |
| File SHA-256 | `2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b` |
| License | OFL-1.1 |
| Age | elapsed 143678385 seconds, 1662 days |

OFL-1.1 obligations that still apply: keep the copyright and license with the font, do not sell the font by itself, and do not use the reserved font name for a modified font. This card does not modify the font. `audited` stays false.

## What this record is not

It is not owner approval. It is not a baseline edit. `closes_sp09_w05` stays false. Transitive crate checksums beyond the four direct pins are the child `Cargo.lock` produced by one `cargo generate-lockfile` in the private builder, then `--locked`. That is not `cargo update` and it is not a root lock change.
