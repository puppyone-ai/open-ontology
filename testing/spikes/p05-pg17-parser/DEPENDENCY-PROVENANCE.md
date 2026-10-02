# P5 parser spike dependency provenance

Point-in-time record for `testing/spikes/p05-pg17-parser`. This crate is not a product dependency. a063 remains the only writer of the root lock.

Age floor: `as_of` 2026-10-01T23:07:52Z, minimum age 604800 seconds. crates.io metadata for all 80 registry packages in `Cargo.lock` matched the lock checksum, none were yanked, and none were younger than the floor. OSV `querybatch` for those 80 crates returned zero vulnerabilities on 2026-10-02. `cargo audit` and `cargo deny` were not run. Installing them would change the host toolchain.

## Parser pin

| Item | Value |
|---|---|
| Crate | `pg_query` 6.2.0, not yanked |
| crates.io created | 2026-08-03T15:35:05.906784Z |
| crates.io checksum | `42a66643c4aa9c929b68b2c9e30d6c4d955bfd1e87e255ce6c5d0c026082436c` |
| License field | MIT |
| Published `rust-version` | none |
| Git | `pganalyze/pg_query.rs` tag `v6.2.0`, commit `0e382742b36ee9592438c6791b91ce14f32f6233` |
| Vendored submodule | `pganalyze/libpg_query` tag `17-6.2.2`, commit `7be1aed1f1f968a36cf541319f71e845850f0381`. GitHub commit author date 2026-01-27T05:39:40Z. The release record used in a065 said published 2026-01-27T05:42:48Z. |
| Embedded PostgreSQL | `PG_MAJORVERSION` 17, `PG_VERSION` 17.7, `PG_VERSION_NUM` 170007, string `PostgreSQL 17.7 (libpg_query)` |

`pg_query` 6.2.1 exists (created 2026-09-30T19:27:10.662249Z, checksum `da43168013d137b3ca49b0044c1108ec09ff596a0626d4db89209c0a1d450cd2`, git `c780e49d095c7c97def832e45ada3f559d5b42cf`). Its submodule `c2c27b1e5b13584b594e5dee082f9d007b5e721b` is libpg_query release 17-6.2.5 and still defines `PG_VERSION` 17.7 / `PG_VERSION_NUM` 170007. It is under the age floor and does not move the parser to 17.10. This spike stays on 6.2.0. The product default PostgreSQL is unchanged.

The PG 17.10 fixture reported `server_version` `17.10 (Debian 17.10-1.pgdg12+1)` and `server_version_num` 170010. Same major, different patch. Official 17.10 notes (2026-05-14) describe fixes from 17.9, not a new major grammar. This spike did not diff every 17.8–17.10 commit. The shared syntax it actually ran on both sides is in the evidence report, including `JSON_TABLE`.

## Direct and build pins

| Crate | Version | crates.io license | `rust-version` | Created |
|---|---|---|---|---|
| serde | 1.0.229 | MIT OR Apache-2.0 | 1.56 | 2026-07-18 |
| serde_json | 1.0.151 | MIT OR Apache-2.0 | 1.71 | 2026-07-20 |
| libc | 0.2.189 | MIT OR Apache-2.0 | 1.65 | 2026-07-21 |
| cc | 1.4.7 | MIT OR Apache-2.0 | 1.65.0 | 2026-09-18 |
| find-msvc-tools | 0.1.13 | MIT OR Apache-2.0 | 1.65.0 | 2026-09-18 |
| lazy_static | 1.5.0 | MIT OR Apache-2.0 | none | 2024-06-21 |
| bindgen | 0.66.1 | BSD-3-Clause | 1.60.0 | 2023-06-19 |
| prost | 0.13.5 | Apache-2.0 | 1.71.1 | 2025-02-12 |

The smoke lock had `cc` 1.5.1, `find-msvc-tools` 0.1.14, and `lazy_static` 1.5.1. Cargo 1.94 inside the pinned builder downgraded them with `cargo update --precise` before `cargo fetch --locked`. Host Cargo 1.98 did not rewrite this lock.

## Native build and MSRV

`pg_query` 6.2.0 builds a static `libpg_query` with the `cc` crate and runs `bindgen` 0.66.1 against the vendored headers. That needs `clang` and `libclang`. The official `rust:1.94.0` image does not ship them. `scripts/linux-locked.sh` installs `clang`, `libclang-dev`, `llvm-dev`, and `pkg-config` when they are missing, and exits 3 if `protoc` is on `PATH` so `build.rs` cannot regenerate `protobuf.rs`. This run printed `protoc_present=no`.

The spike `rust-version` is 1.94. The Linux proof is `rustc 1.94.0 (4a4ef493e 2026-03-02)` `aarch64-unknown-linux-gnu`. A published `rust-version` of 1.71 on a dependency is not a claim that this C build works below 1.94. Host checks used Homebrew `rustc 1.98.1 (48a229cea 2026-09-01)` and are not the MSRV proof.

The Linux smoke binary's `ldd` is `linux-vdso.so.1`, `libgcc_s.so.1`, `libc.so.6`, and `ld-linux-aarch64.so.1`. It does not link `libclang`. A later container with `--network none` and `--cap-drop ALL` ran that binary and printed `parser_tree_version=170007`, `deparsed=SELECT 1`, exit 0.

## License bodies

SHA256 of the files in `licenses/`. These are the bodies this spike recorded. They are not a blanket grant.

| File | SHA256 | What it is |
|---|---|---|
| `pg_query.rs-6.2.0-LICENSE` | `39fac7e7daa7cf3173d96f9e8a167f980b93b90386b83c342de70db3bc7bbb68` | MIT. Copyright 2021 Paul Mason; 2021–2023 Duboce Labs. |
| `libpg_query-17-6.2.2-LICENSE` | `f9352c20940619a380a5e9f23cebe34d8f797cfbd747fc236a7a3e5fc8ef9b41` | BSD-3-Clause. Copyright 2015 Lukas Fittl; 2016–2023 Duboce Labs. |
| `postgresql-17.7-COPYRIGHT` | `5ed3ce5c9373dff7f98b1fae7a6c7ccd98df7d734d46d24c1bcebf1240be8307` | PostgreSQL license. Portions Copyright 1996–2025 PostgreSQL Global Development Group; 1994 The Regents of the University of California. |
| `xxhash-bsd-2-clause-header.txt` | `20bf2c9b132fe78c4256abb204159403d987af7a45345f1bdde07ecc094bed7f` | BSD-2-Clause header. Copyright 2012–2020 Yann Collet. |
| `protobuf-c-bsd-header.txt` | `142a052f7d22404fb513ad9f8d03f78f27e797352292c3a60c5c2babcef8285e` | BSD-like 2-clause header excerpt. Copyright 2008–2018 Dave Benson and the protobuf-c authors, matching the compiled `protobuf-c.h`. |
| `bison-2.3-skeleton-exception-excerpt.txt` | `e8ecf341ce94fb7ad19de4a4985374eb821ff84fd4dff943b10a022ca93fc7a3` | 94-line excerpt of the generated Bison 2.3 skeleton in `gram.c`, through the special-exception paragraph. GPL-2.0-or-later. The exception allows a larger work that contains the skeleton to use other terms, as long as that work is not itself a parser generator using the skeleton. Modifying or redistributing the skeleton itself may drop the exception, and the skeleton is then GPL without it. This is not a blanket GPL exception for PostgreSQL or for this spike. |

## Upstream texts checked for a080

The extracted `pg_query` 6.2.0 crate contains no `LICENSE`, `COPYING`, or `COPYRIGHT` file. That is not evidence that the upstream projects are unlicensed. On 2026-10-02 these URLs returned HTTP 200 and the bytes below.

| Source | SHA256 | Comparison |
|---|---|---|
| `https://raw.githubusercontent.com/pganalyze/pg_query.rs/0e382742b36ee9592438c6791b91ce14f32f6233/LICENSE` | `39fac7e7daa7cf3173d96f9e8a167f980b93b90386b83c342de70db3bc7bbb68` | Byte-equal to `licenses/pg_query.rs-6.2.0-LICENSE`. |
| `https://raw.githubusercontent.com/pganalyze/libpg_query/7be1aed1f1f968a36cf541319f71e845850f0381/LICENSE` | `f9352c20940619a380a5e9f23cebe34d8f797cfbd747fc236a7a3e5fc8ef9b41` | Byte-equal to `licenses/libpg_query-17-6.2.2-LICENSE`. |
| `https://raw.githubusercontent.com/postgres/postgres/REL_17_7/COPYRIGHT` | `5ed3ce5c9373dff7f98b1fae7a6c7ccd98df7d734d46d24c1bcebf1240be8307` | Byte-equal to `licenses/postgresql-17.7-COPYRIGHT`. |
| `https://raw.githubusercontent.com/pganalyze/libpg_query/7be1aed1f1f968a36cf541319f71e845850f0381/vendor/protobuf-c/protobuf-c.h` | `3c06432ca3233a730e7b20afc1bc5750dd3f8c298db9c99b5df03dc6e1a4740c` | Byte-equal to `vendor/protobuf-c/protobuf-c.h` inside the crates.io crate. `licenses/protobuf-c-bsd-header.txt` is a prefix of that header. The copyright line is `2008-2018`. |

The GitHub commit API object for `7be1aed1f1f968a36cf541319f71e845850f0381` says message `Release 17-6.2.2 (#326)`, author Lukas Fittl, author date `2026-01-27T05:39:40Z`, `verification.verified=true`, `verification.reason=valid`.

`xxhash` was not re-fetched in a080. Its recorded header hash is unchanged.

This match of license bytes and of the claimed libpg_query commit is not a full supply-chain approval. `cargo audit` and `cargo deny` were not run. An OSV result of zero hits is not an approval. The age floor is unchanged. The Bison excerpt hash is unchanged, and the special-exception limits in the table above still apply.
