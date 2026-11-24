# Super Ontology S1 dependency delta (B-S1-07)

Adoption time 2026-11-17T10:35:12Z. Seven-day cutoff 2026-11-10T10:35:12Z: every version below was published at or before it. Plan `SO-B-S1-07`; spec 10 sections 2.4 and 2.5, spec 90 section 2.

This file is the index for the six records `blake3-audit.json`, `pulldown-cmark-audit.json`, `calamine-audit.json`, `opendal-audit.json`, `proptest-audit.json` and `unicode-normalization-audit.json`. `dependency-baseline.json` and `dependency-baseline.md` are the frozen SP00-W01 document and are not edited here, in line with the crossterm, ratatui, zstd and webpki admissions that also left them untouched. Each package has all 14 policy fields in its record (top-level package plus `native_transitive` entries); `governance/qa_record.py audited_record_errors` returns no errors for any of the six.

## Chosen top-level versions

| Crate | Version | Published (index pubtime) | License relied on | Features | Where |
|---|---|---|---|---|---|
| blake3 | 1.8.7 | 2026-08-20T09:09:56Z | Apache-2.0 | `std`, `pure`; default off | crates/oo-connectors/Cargo.toml (optional, feature objectstore); crates/oo-docs/Cargo.toml |
| pulldown-cmark | 0.13.6 | 2026-10-30T08:50:18Z | MIT | none; default off (no html, getopts, simd) | crates/oo-docs/Cargo.toml |
| calamine | 0.38.0 | 2026-11-02T01:32:22Z | MIT | none (crate has no default) | crates/oo-docs/Cargo.toml |
| opendal | 0.59.5 | 2026-10-18T05:36:20Z | Apache-2.0 | `services-s3` only; default off | crates/oo-connectors/Cargo.toml (optional, feature objectstore-s3) |
| proptest | 1.12.0 | 2026-10-10T18:18:05Z | Apache-2.0 | `std`; default off (no fork, timeout, bit-set); dev-only | crates/oo-docs/Cargo.toml ([dev-dependencies]) |
| unicode-normalization | 0.1.25 | 2025-10-30T16:06:38Z | Apache-2.0 | default (already locked) | crates/oo-connectors/Cargo.toml; crates/oo-docs/Cargo.toml |

## All 34 new packages in Cargo.lock (plus 2 record-only)

| Package | Version | Published | Relied on | Record | Build script |
|---|---|---|---|---|---|
| blake3 | 1.8.7 | 2026-08-20T09:09:56Z | Apache-2.0 | blake3-audit.json | yes |
| arrayvec | 0.7.8 | 2026-07-02T12:44:35Z | Apache-2.0 | blake3-audit.json | no |
| constant_time_eq | 0.4.2 | 2025-03-15T17:03:47Z | Apache-2.0 | blake3-audit.json | no |
| pulldown-cmark | 0.13.6 | 2026-10-30T08:50:18Z | MIT | pulldown-cmark-audit.json | yes |
| unicase | 2.10.0 | 2026-10-05T13:11:23Z | Apache-2.0 | pulldown-cmark-audit.json | no |
| calamine | 0.38.0 | 2026-11-02T01:32:22Z | MIT | calamine-audit.json | no |
| atoi_simd | 0.18.1 | 2026-02-09T12:24:52Z | Apache-2.0 | calamine-audit.json | no |
| codepage | 0.1.3 | 2026-09-12T17:15:28Z | Apache-2.0 | calamine-audit.json | no |
| constant_time_eq | 0.6.1 | 2026-10-01T00:18:04Z | Apache-2.0 | calamine-audit.json | yes |
| debug_unsafe | 0.1.4 | 2026-02-09T12:18:49Z | Apache-2.0 | calamine-audit.json | no |
| fast-float2 | 0.2.4 | 2026-08-14T02:30:24Z | Apache-2.0 | calamine-audit.json | no |
| quick-xml | 0.42.0 | 2026-08-22T19:22:01Z | MIT | calamine-audit.json | no |
| typed-path | 0.13.0 | 2026-10-20T03:16:20Z | Apache-2.0 | calamine-audit.json | no |
| zip | 9.5.0 | 2026-11-09T00:03:35Z | MIT | calamine-audit.json | no |
| zlib-rs | 0.6.8 | 2026-09-15T13:10:00Z | Zlib | calamine-audit.json | no |
| zopfli | 0.8.4 | 2026-10-11T19:45:43Z | Apache-2.0 | calamine-audit.json | no |
| opendal | 0.59.5 | 2026-10-18T05:36:20Z | Apache-2.0 | opendal-audit.json | no |
| opendal-core | 0.59.5 | 2026-10-18T05:31:48Z | Apache-2.0 | opendal-audit.json | no |
| opendal-service-s3 | 0.59.5 | 2026-10-18T05:35:06Z | Apache-2.0 | opendal-audit.json | no |
| asyncband | 0.7.4 | 2026-10-25T07:24:12Z | Apache-2.0 AND MIT | opendal-audit.json | no |
| crc-fast | 1.10.0 | 2025-12-31T19:34:28Z | Apache-2.0 | opendal-audit.json | no |
| dlv-list | 0.5.2 | 2023-10-24T20:28:09Z | Apache-2.0 | opendal-audit.json | no |
| hashbrown | 0.14.5 | 2024-04-28T18:32:09Z | Apache-2.0 | opendal-audit.json | no |
| ordered-multimap | 0.7.3 | 2024-04-01T10:25:43Z | MIT | opendal-audit.json | no |
| reqsign-aws-core | 3.2.0 | 2026-09-25T09:21:26Z | Apache-2.0 | opendal-audit.json | no |
| reqsign-aws-v4 | 3.3.1 | 2026-09-25T09:21:28Z | Apache-2.0 | opendal-audit.json | no |
| reqsign-core | 3.3.2 | 2026-09-25T09:21:14Z | Apache-2.0 | opendal-audit.json | no |
| reqsign-file-read-tokio | 3.0.7 | 2026-09-25T09:21:18Z | Apache-2.0 | opendal-audit.json | no |
| rust-ini | 0.21.3 | 2025-08-30T17:27:36Z | MIT | opendal-audit.json | no |
| serde_urlencoded | 0.7.1 | 2022-01-17T11:26:21Z | Apache-2.0 | opendal-audit.json | no |
| spin | 0.10.1 | 2026-07-13T19:46:55Z | MIT | opendal-audit.json | no |
| proptest | 1.12.0 | 2026-10-10T18:18:05Z | Apache-2.0 | proptest-audit.json | no |
| rand_xorshift | 0.5.0 | 2026-02-02T08:15:36Z | Apache-2.0 | proptest-audit.json | no |
| unarray | 0.1.4 | 2022-11-30T17:09:42Z | Apache-2.0 | proptest-audit.json | no |
| unicode-normalization | 0.1.25 | 2025-10-30T16:06:38Z | Apache-2.0 | unicode-normalization-audit.json | no |
| tinyvec | 1.13.3 | 2026-09-13T23:30:12Z | Apache-2.0 | unicode-normalization-audit.json | no |

## Decisions

- blake3 uses `pure`. Without it, the build script compiles C and assembly through `cc` on x86-64 and C NEON intrinsics on aarch64 (both release targets of `ci/deny.toml`). With it, nothing native is built or linked; the cost is no AVX-512 and no NEON. `cc` is already locked, so no new native build dependency exists either way.
- zip 9.6.1 and 9.6.0 are younger than seven days; zip 9.5.0 is the newest release that clears the floor. nix 0.32.0 (pulled by zip 9.6.1) is not in the lock; nix 0.31.3 is reused.
- calamine stays in `oo-docs` now: its 11-package set is plain Rust, all permissive, none with native build code, so there was no reason to defer it to B-S3-02.
- opendal with only `services-s3` has no HTTP transport, retry layer or tokio executor. The five extra packages that B-S4-01 will need if it uses OpenDAL's own are listed in the opendal record notes; they are not in this lock and need their own records.
- unicode-normalization is already in the lock at 0.1.25; B-S1-07 only records it and adds `oo-docs` as a selector.
- constant_time_eq appears twice (0.4.2 via blake3, 0.6.1 via zip) and hashbrown 0.14.5 is a second hashbrown; `cargo deny` reports them as `multiple-versions = warn`.

## Scan result

`cargo audit` (0.22.2, advisory-db 233155b4025b854c68a4b199c0e32d7fd53291a2): 0 vulnerabilities, 644 crates, no warning names a package in this change. `cargo deny` (0.20.2, `ci/deny.toml`): bans ok, licenses ok, sources ok; advisories fail on three pre-existing unmaintained notices (chrono, chrono-tz, humantime) that fail identically on the base commit. Output is under `supply-chain/evidence/b-s1-07/`.

## Existing packages whose edges changed

Re-resolution moved the Windows-only `windows-sys` edge of errno, rustix, quinn-udp, rustls-platform-verifier, tempfile and winapi-util from 0.61.2 to 0.52.0 in `Cargo.lock`. Both versions were already locked; no version was added, removed or upgraded.

