# a069 review: ISC allowlist commit

Decision: **request_changes**. The change is limited to the wording that says `r-efi` 6.0.0 ships no MIT text. The `ISC` allow entry stays. This review does not revert it, does not edit `supply-chain/policy.md`, and does not ask a063, a066, or any other product lane to stop.

Reviewed commit `b14c7821031990aafd713f23068658d41042ee2d`, tree `9e5bf8d0edb33a141cb3b260c924f3c614afc123`, parent `201708d7160f269e3c3cecccd7b517efdd7fcfd2`. The commit is an ancestor of `master`. The review used `git archive` of that commit and of immutable `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74`. It did not check out either commit and did not edit source, `ci/deny.toml`, contracts, locks, or old evidence.

## What the diff is

`git diff --name-status` from the parent is 123 paths. Every path is `ci/deny.toml`, `contracts/supply-chain/isc/`, or `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a067/`. The deny diff is one inserted line:

```
 allow = [
     "Apache-2.0",
     "MIT",
+    "ISC",
     "BSD-3-Clause",
```

Parent blob `a0a28e378bee2ae474a4ef0cdc8a46c465354dab`, sha256 `ee87f177fbf221221c5f3a7f0020dc393a047da9af77ec797c2953fc25136559`. New blob `367598cb3768b8c8af698041cd0f84ce6f8fb7be`, sha256 `4320807bfe747fe339ce7bf539a1cff5604e8e0c7677760e2cbc7abc1868624b`. The archive copy of `ci/deny.toml` matches the new blob. `b2d3cbc:ci/deny.toml` matches the parent blob.

Unchanged in that file: `confidence-threshold = 0.8`, `unused-allowed-license = "warn"`, `yanked = "deny"`, `unmaintained = "all"`, `unsound = "all"`, `ignore = []`, `unknown-registry = "deny"`, `unknown-git = "deny"`, crates.io as the only registry, `multiple-versions = "warn"`, `wildcards = "deny"`. No `exceptions` list. No `clarify` block. `CDLA-Permissive-2.0` is not added. `GPL` is not added.

## Blocking finding

`contracts/supply-chain/isc/NOTICE.md` says `r-efi` 6.0.0 has no MIT license text in the crate and that `AUTHORS` is not that license. `policy.md` says deny's acceptance does not mean the MIT text is in the crate, and that `fixtures/mit-or-gpl-missing-mit-text` is the synthetic form of the `r-efi` fact. `HANDOFF.md` and `REPORT.md` call the missing-text fixture the `r-efi` 6.0.0 situation.

The published crate, downloaded from `https://static.crates.io/crates/r-efi/r-efi-6.0.0.crate` and matched to the b2 lock checksum `f8dcc9c7d52a811697d2151c701e0d08956f92b0e24136cf4cf27b57a6a0d9bf`, has no file whose name is `LICENSE`, `COPYING`, or `NOTICE`. `README.md` sha256 `420f65fec3ba2e89d49da51b222d2881d26959037a7b7d3794143db4bd4627b1` names the expression as markdown `**MIT** OR **Apache-2.0** OR **LGPL-2.1-or-later**` and does not contain the MIT grant. `AUTHORS` sha256 `d027e91dbc9cdbb2f1190068e498bd6b61cff022b6a032b191021ba658d96111` is the hash this commit already published, and the file does contain the MIT grant under the heading `AUTHORS-MIT`. Later in the same file:

```
COPYRIGHT: (ordered alphabetically)
        Copyright (C) 2017-2023 Red Hat, Inc.
        Copyright (C) 2019-2023 Microsoft Corporation
        Copyright (C) 2022-2023 David Rheinsberg
```

An `AUTHORS:` name list follows those lines. Those names were read from the crate. They were not invented. The MIT grant says "the above copyright notice" and the copyright lines sit below the grant, not above it. `AUTHORS-ASL` is the short Apache pointer to `http://www.apache.org/licenses/LICENSE-2.0`, not the full Apache-2.0 text. `AUTHORS-LGPL` is the short LGPL pointer, not the full LGPL text.

`r-efi` 5.3.0 checksum `69cdb34c158ceb288df11e18b4bd39de994f6657d83847bdffdbd7f346754b0f` was opened here. a067 said that tree was not opened. Its `README.md` is byte-identical to 6.0.0. Its `AUTHORS` sha256 is `ff92bed461f50338dd703a9ba9aee496a425957873df1d91192776e4bdf5dda7` and begins with the same MIT grant. The two `AUTHORS` files are not the same length.

The synthetic fixture is still a real tool gap: deny exits 0 for `MIT OR GPL-3.0-only` when neither license text is in the tree. That fixture is not the `r-efi` crate. A distribution that selects MIT for `r-efi` can point at the grant and the copyright lines in `AUTHORS`. It still cannot point at a file named `LICENSE`.

Required correction, in this slice's notice and policy text only: state the `AUTHORS-MIT` grant, the three copyright lines, the author list's presence, and the Apache and LGPL pointers. Keep the filename fact. Stop calling the missing-text fixture the `r-efi` fact. Do not remove `"ISC"`. Do not add `CDLA-Permissive-2.0`. Do not edit `supply-chain/policy.md`.

A smaller attribution slip can be fixed in the same pass. `REPORT.md` glosses rustls `LICENSE-ISC` as `Copyright (c) 2016 Joseph Birr-Pixton`. The file's second line is `Copyright (c) 2016, Joseph Birr-Pixton <jpixton@gmail.com>`, with a comma. The file sha256 `7cfafc877eccc46c0e346ccbaa5c51bb6b894d2b818e617d970211e232785ad4` matches provenance. `NOTICE.md` quotes the MIT file's no-comma line and does not misquote the ISC line. Ship the file.

## ISC behavior that was confirmed

cargo-deny 0.20.2, crate sha256 `e528dfcbe739af7ce37a77d3d6df1b29dd6887b1c701d888820c0f16b864f737`, matches `crates/xtask/src/supply.rs` in the archive and the copy `cargo install --locked` placed in this run's `CARGO_HOME`. crates.io `created_at` is `2026-07-09T17:58:09.303885Z`; the pin drops the fraction and records `2026-07-09T17:58:09Z`. Not yanked. Host install compiler was rustc 1.98.1 `(48a229cea 2026-09-01)`, Homebrew, Mach-O arm64. Binary sha256 `166d44d04aba1f87e9c8f540c7852ef9faf5ca1efc2d5877a963e773be769a14`. Version string `cargo-deny 0.20.2`.

In `src/cargo-deny/stats.rs`, `stats_to_exit_code` folds `[advisories, bans, licenses, sources]` and sets bit `i` when that check has errors. License errors alone produce `1 << 2`, which is 4. Exit 4 is not a count of four errors. A license failure plus a ban failure would be 6.

On the b2 archive, `--locked check`:

| Config | Exit | License errors | Other checks |
|---|---|---|---|
| Parent allow list, JSON and human | 4 | 4: `ring` 0.17.14 `Apache-2.0 AND ISC` / `ISC`; `rustls-webpki` 0.103.15 `ISC`; `untrusted` 0.9.0 `ISC`; `webpki-roots` 1.0.9 `CDLA-Permissive-2.0` | advisories 0, bans 0 errors and 12 warnings, sources 0. JSON stderr 48094 bytes. Human stdout `advisories ok, bans ok, licenses FAILED, sources ok`. |
| ISC allow list, JSON and human | 4 | 1: `webpki-roots` 1.0.9 `CDLA-Permissive-2.0` | advisories 0, bans 0 errors and 12 warnings, sources 0. Helps 241 to 244. JSON stderr 45183 bytes. |

The three ISC errors close. CDLA remains. The process still exits 4. That is not an all-supply-chain pass.

The commit's own lock, sha256 `636e19b8761bd4f212381fd0abe6a3040ca6ee8024aed06dd350a1fd55ec32d1`, contains no `name = "ring"`. cargo-deny on that archive exits 0 with both configs. The new config adds one license warning, `license-not-encountered` span `ISC`. Bans stay at one duplicate-`syn` warning. Advisories 0. Sources 0. Human stdout is `advisories ok, bans ok, licenses ok, sources ok`. That pass is the foundation lock. It does not prove the b2 ISC errors closed.

Committed fixtures, same binary, old config then new config:

| Fixture | Expression | Old exit / license errors | New exit / license errors |
|---|---|---|---|
| positive-isc | `ISC` | 4 / 1 | 0 / 0 |
| negative-gpl-only | `GPL-3.0-only` | 4 / 1 | 4 / 1 |
| negative-agpl-only | `AGPL-3.0-only` | 4 / 1 | 4 / 1 |
| negative-unknown | `LicenseRef-OO-Synthetic-Unknown` | 4 / 1 | 4 / 1 |
| negative-bsl-only | `BUSL-1.1` | 4 / 1 | 4 / 1 |
| negative-boost-bsl-only | `BSL-1.0` | 4 / 1 | 4 / 1 |
| negative-sspl-only | `SSPL-1.0` | 4 / 1 | 4 / 1 |
| negative-llvm-exception-only | `Apache-2.0 WITH LLVM-exception` | 4 / 1 | 4 / 1 |
| dual-mit-or-gpl | `MIT OR GPL-3.0-only` | 0 / 0 | 0 / 0 |
| exception-or-apache-mit | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | 0 / 0 | 0 / 0 |
| mit-or-gpl-missing-mit-text | `MIT OR GPL-3.0-only`, neither text present | 0 / 0 | 0 / 0 |

`BUSL-1.1` is the Business Source License. `docs/10` §11 and `supply-chain/policy.md` §3 keep it out of the default image. `BSL-1.0` is the Boost Software License 1.0. whoami 2.1.3 declares `Apache-2.0 OR BSL-1.0 OR MIT` and ships `LICENSE_BOOST`, which begins `Boost Software License - Version 1.0 - August 17th, 2003`. That file is not a Business Source license. Rejecting a BSL-1.0-only fixture is the allow list's current scope. It is not a finding that Boost is nonpermissive. The selected whoami licenses remain MIT or Apache-2.0.

## Apache AND ISC control

Independent probe `raw/probes/apache-and-isc`, expression `Apache-2.0 AND ISC`. `LICENSE-ISC` is present. `APACHE-TEXT-ABSENT` says the Apache-2.0 text is not in the crate. Old config exit 4, spans `Apache-2.0 AND ISC` and `ISC`. New config exit 0. Allowing ISC satisfies the ISC side of the AND. It does not replace the Apache-2.0 side, and the exit 0 did not look for an Apache body.

On the real graph, `ring` 0.17.14 declares `Apache-2.0 AND ISC`. The new config no longer errors on it. The crate still contains `LICENSE`, `LICENSE-other-bits`, `LICENSE-BoringSSL`, `src/polyfill/once_cell/LICENSE-APACHE`, `src/polyfill/once_cell/LICENSE-MIT`, and `third_party/fiat/LICENSE`. Those files are still the distribution duty. An accepted SPDX expression is not a copy of the license body.

## Registry bytes

Each `.crate` below was fetched from `static.crates.io`. sha256 matches the b2 `Cargo.lock` checksum and the crates.io API `checksum`. None of these versions were yanked. API `created_at` values were read on this run.

| Crate | Version | sha256 | created_at | Manifest license |
|---|---|---|---|---|
| ring | 0.17.14 | `a4689e6c2294d81e88dc6261c768b63bc4fcdb852be6d1352498b114f61383b7` | 2025-03-11T19:42:03.889273Z | `Apache-2.0 AND ISC` |
| rustls-webpki | 0.103.15 | `f3c3cf1d8b1e7d4927e2d154c3fcb02979afb9939629c62cd9048d4f07b60ac2` | 2026-08-21T16:40:00.687282Z | `ISC` |
| untrusted | 0.9.0 | `8ecb6da28b8a351d773b68d5825ac39017e680750f980f3a1a85cd8dd28a47c1` | 2021-07-13T22:13:26.650010Z | `ISC` |
| rustls | 0.23.45 | `0d41d731c7d2f962d1ccc364cec258de3c0e93b38c2fb3ba97ac74513048d634` | 2026-09-14T15:11:17.808465Z | `Apache-2.0 OR ISC OR MIT` |
| webpki-roots | 1.0.9 | `7dcd9d09a39985f5344844e66b0c530a33843579125f23e21e9f0f220850f22a` | 2026-07-18T09:30:44.802479Z | `CDLA-Permissive-2.0` |
| whoami | 2.1.3 | `626c4bac6755d76ffc12cb01b2eac751db1996b9e0041de9aa02c8c211ddc82c` | 2026-08-11T23:31:57.132972Z | `Apache-2.0 OR BSL-1.0 OR MIT` |
| wasi | 0.11.1+wasi-snapshot-preview1 | `ccf3ec651a847eb01de73ccad15eb7d99f80485de043efb2f370cd654f4ea44b` | 2025-06-10T14:23:45.376914Z | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` |
| r-efi | 6.0.0 | `f8dcc9c7d52a811697d2151c701e0d08956f92b0e24136cf4cf27b57a6a0d9bf` | 2026-02-03T09:53:46.251122Z | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` |
| r-efi | 5.3.0 | `69cdb34c158ceb288df11e18b4bd39de994f6657d83847bdffdbd7f346754b0f` | 2025-06-17T09:12:38.227277Z | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` |

License-file sha256 values in `provenance.md` match these tarballs, 20 of 20, including ring BoringSSL, fiat, and once_cell. The four fenced bodies in `NOTICE.md` (rustls-webpki `LICENSE` 916 bytes, untrusted `LICENSE.txt` 769 bytes, ring `LICENSE` 499 bytes, ring `LICENSE-other-bits` 731 bytes) are byte-identical to the crate files.

ring has no `NOTICE` file and no `AUTHORS` file. `third_party/fiat/LICENSE` says `Copyright 2015-2020 the fiat-crypto authors (see the AUTHORS file)`. That AUTHORS file is not in the crate. No author list was invented for it. `LICENSE-BoringSSL` is 14870 bytes, contains the Apache-2.0 text, and its support-code section says the Go TLS test-suite license is not triggered by distributing code linked against BoringSSL. The once_cell MIT file is the permission text and has no `Copyright` owner line. `race.rs` in that directory has no copyright header. rustls `LICENSE-APACHE`, ring once_cell `LICENSE-APACHE`, and wasi `LICENSE-APACHE` are the same sha256 `a60eea817514531668d7e00765731449fe14d059d3249e0bc93b36de45f759f2`. rustls-webpki `LICENSE` still mentions `third-party/chromium/LICENSE`. The published crate has no `third-party` directory. webpki-roots `LICENSE` is the CDLA-Permissive-2.0 agreement. It stays unapproved.

whoami and wasi publish times were not in a067's table. They are in the API rows above. The crate checksums already matched the lock.

## OR-token rule

`supply-chain/policy.md` §2 says GPL-2.0 and GPL-3.0 stay out of the default artifact even when an `OR` expression also names a permissive license. §3 says a prohibited token blocks the default record even as one side of an `OR`. `ci/deny.toml` does not implement that token scan. It did not implement it before this commit. `dual-mit-or-gpl` exits 0 on the parent config.

Independent probe `raw/probes/isc-or-gpl`, expression `ISC OR GPL-3.0-only`, ISC text present, GPL text absent: old config exit 4 with spans `ISC OR GPL-3.0-only`, `ISC`, and `GPL-3.0-only`; new config exit 0. Adding ISC means an expression whose only allowed side is ISC now passes deny even if GPL is the other side. That is the pre-existing OR rule applied to the new identifier. It is not a new confidence, advisory, or source setting.

This review does not treat deny's exit 0 as a waiver of `supply-chain/policy.md`. The audit-record rule still rejects a GPL token inside an `OR`. The commit does not edit that file, and this review does not ask for that edit as a condition of the ISC line. Reconciling the two checkers is separate work. It is not a reason to stop unrelated product work, and it is not a reason to remove ISC from crates that are ISC-only or `Apache-2.0 AND ISC`.

## Linux arm64 Rust 1.94 smoke

One builder, this task's container `oo-a069-msrv-729d7e89-c060-489e-be7e-f1c08c2f40c8`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `--pull never`, `--platform linux/arm64`. Inspect: exit 0, oom false, network bridge, privileged false, user `0:0`, pids limit 4096, memory 8589934592, CapDrop ALL, security `no-new-privileges`. Binds were the archive read-only, the smoke script read-only, this evidence directory, and task-owned cargo home and target. No host home, no docker socket, no host network. `HostConfig.Tmpfs` is null.

Inside the container, `RUSTUP_TOOLCHAIN=1.94.0`. `rustc -Vv` printed `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. `cargo 1.94.0 (85eff7c80 2026-01-15)`. The archive's `rust-toolchain.toml` says 1.98.1; the environment override selected 1.94.0. Clippy for 1.94.0 was installed in the container, phase exit 0.

The commit changes no Rust sources, so the focused package was `oo-core`, using the command shape in `execution/prompts/linux-msrv-focused-checks.md`.

| Phase | Exit |
|---|---|
| `cargo fetch --manifest-path Cargo.toml --locked` | 0 |
| `cargo test --manifest-path Cargo.toml --locked --offline -p oo-core` | 0 |
| `cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-core --all-targets -- -D warnings` | 0 |

119 `Downloaded` lines, all before the fetch marker. The test marker still shows 119, so the offline test and clippy did not download crates. Test totals: 11, 11, 8, 5, 19, 8, and 0, which is 62 passed and 0 failed. `elapsed_seconds=50`. Docker exit 0. This is one architecture and one package. It is not a matrix, not an air-gap claim, and not SP00-W06.

A first launch was not started. The capacity count matched the shell's own command line, which contained the word cargo. The recount by process name found no cargo or rustc, and no container labeled `oo.role=rust-builder`. The smoke above is the run that executed. At that moment the other containers were the a068 and a066 Postgres fixtures, not Rust builders.

## What this review does not approve

SP00-W06, AT-SEC05, protected CI, TLS, a private CA, source registration, customer delivery, and release are not approved. `CDLA-Permissive-2.0` remains a license error on `b2d3cbc`. Exit 4 remains.

The PostgreSQL checkout at `/Users/redux/.codex/worktrees/product-pg/open-ontology` was `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74` before and after `git archive`, with 19 porcelain lines both times. A later `rev-parse`, after this review's deny script had finished, returned `181df9b6ba030b68e3a48aa0f58e5c5f909bee55`. This review did not check out that commit and did not move the checkout back.

Raw exits, stderr, and the parsed summary are under `raw/deny/`. License comparisons are in `raw/license-bodies.json` and `raw/crate-fetch.json`. The smoke log is `linux/console.log`.

The reviewer did not commit. The coordinator stages this report.
