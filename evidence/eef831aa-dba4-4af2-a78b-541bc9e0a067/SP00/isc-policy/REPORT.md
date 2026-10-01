# a067 ISC allowlist

Session `eef831aa-dba4-4af2-a78b-541bc9e0a067`. Run `32680b16-6285-466e-a244-7f50e87e2b44`.

`ci/deny.toml` gains one allow entry, `ISC`. The parent of this commit is `raw/parent.txt`, written from `git rev-parse HEAD` immediately before `git commit`. This report does not name its own commit object.

This is the foundation allowlist change for the ISC license required by `ring` 0.17.14, `rustls-webpki` 0.103.15, and `untrusted` 0.9.0 on source commit `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74`. Attribution and license-distribution duties stay in force. `CDLA-Permissive-2.0` stays off the allow list. SP00-W06 is unfinished. On that source graph, cargo-deny still exits 4 because `webpki-roots` 1.0.9 remains a license error. Protected CI, release approval, source registration, and TLS acceptance are outside this commit.

## Policy diff

Working tree `ci/deny.toml` sha256 `4320807bfe747fe339ce7bf539a1cff5604e8e0c7677760e2cbc7abc1868624b`. The file at `HEAD` before this commit, and the `b2d3cbc` archive copy, are sha256 `ee87f177fbf221221c5f3a7f0020dc393a047da9af77ec797c2953fc25136559` (git blob `a0a28e378bee2ae474a4ef0cdc8a46c465354dab`).

```
 allow = [
     "Apache-2.0",
     "MIT",
+    "ISC",
     "BSD-3-Clause",
```

`confidence-threshold` stays `0.8`. `unused-allowed-license` stays `warn`. `[advisories] yanked = "deny"`, `unmaintained = "all"`, `unsound = "all"`, `ignore = []`. `[sources] unknown-registry = "deny"`, `unknown-git = "deny"`, crates.io only. The file has no `exceptions` list and no `clarify` block. `raw/policy.diff` is that one insertion.

## Selected crates

Bytes were checked against `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74:Cargo.lock` and against the cargo-deny 0.20.2 registry cache used for that archive. Publish times are the crates.io API on 2026-10-02. None of these versions were yanked. Full notice text is in `contracts/supply-chain/isc/NOTICE.md`. File hashes are in `contracts/supply-chain/isc/provenance.md`.

| Crate | Version | Crate sha256 | SPDX | Selected obligation |
|---|---|---|---|---|
| ring | 0.17.14 | `a4689e6c2294d81e88dc6261c768b63bc4fcdb852be6d1352498b114f61383b7` | `Apache-2.0 AND ISC` | Both licenses. Published 2025-03-11T19:42:03.889273Z. |
| rustls-webpki | 0.103.15 | `f3c3cf1d8b1e7d4927e2d154c3fcb02979afb9939629c62cd9048d4f07b60ac2` | `ISC` | ISC notice in `LICENSE`. Published 2026-08-21T16:40:00.687282Z. |
| untrusted | 0.9.0 | `8ecb6da28b8a351d773b68d5825ac39017e680750f980f3a1a85cd8dd28a47c1` | `ISC` | ISC notice in `LICENSE.txt`, comment form. Published 2021-07-13T22:13:26.650010Z. |
| rustls | 0.23.45 | `0d41d731c7d2f962d1ccc364cec258de3c0e93b38c2fb3ba97ac74513048d634` | `Apache-2.0 OR ISC OR MIT` | Apache-2.0. MIT is an allowed alternative. ISC is unselected. All three texts ship. Published 2026-09-14T15:11:17.808465Z. Deny accepted this expression before ISC was added. |
| webpki-roots | 1.0.9 | `7dcd9d09a39985f5344844e66b0c530a33843579125f23e21e9f0f220850f22a` | `CDLA-Permissive-2.0` | Still rejected. Published 2026-07-18T09:30:44.802479Z. This commit does not admit the data or the identifier. |
| whoami | 2.1.3 | `626c4bac6755d76ffc12cb01b2eac751db1996b9e0041de9aa02c8c211ddc82c` | `Apache-2.0 OR BSL-1.0 OR MIT` | MIT or Apache-2.0. `LICENSE_BOOST` is the Boost Software License 1.0 (SPDX `BSL-1.0`). Publish time was not re-fetched. |
| wasi | 0.11.1+wasi-snapshot-preview1 | `ccf3ec651a847eb01de73ccad15eb7d99f80485de043efb2f370cd654f4ea44b` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | Apache-2.0 or MIT. The LLVM-exception waiver of Apache 4(a), 4(b), and 4(d) is unselected. Publish time was not re-fetched. |
| r-efi | 6.0.0 | `f8dcc9c7d52a811697d2151c701e0d08956f92b0e24136cf4cf27b57a6a0d9bf` | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | Deny accepts the expression. The crate has no `LICENSE`, `COPYING`, or `NOTICE` file. `README.md` is not the MIT text. Published 2026-02-03T09:53:46.251122Z. |

`r-efi` 5.3.0 (`69cdb34c158ceb288df11e18b4bd39de994f6657d83847bdffdbd7f346754b0f`, published 2025-06-17T09:12:38.227277Z) is in the same lock with the same expression and was not a license error. Its tree was not opened.

License-file sha256:

| File | sha256 |
|---|---|
| ring `LICENSE` (499 bytes, pointer) | `b3d734001a94efff3579978d953391aa7115f877657d25eb54037a43875d078a` |
| ring `LICENSE-other-bits` (731 bytes, ISC, Copyright 2015-2025 Brian Smith, "THE AUTHOR") | `f025ccfb7dfb6bdfedc75ca0f67acc69e6fb4998143d834f7c2f38a29989680f` |
| ring `LICENSE-BoringSSL` (14870 bytes; ship the whole file, including the trailing BSD-style notices) | `005fc765ddc5115da796cca915baa9557abae13ff35e0a47c47affc56f6c414d` |
| ring `src/polyfill/once_cell/LICENSE-APACHE` | `a60eea817514531668d7e00765731449fe14d059d3249e0bc93b36de45f759f2` |
| ring `src/polyfill/once_cell/LICENSE-MIT` (no copyright line) | `6ee2ed6c77710de911761acd5fc1ad1da00f476beb1a7ef27e78c2d1858deafc` |
| ring `third_party/fiat/LICENSE` (names AUTHORS; that file is absent) | `9eacbcb81be660840c714a560a9d65ba07913db98dd4baf969f78dd499fdd60f` |
| rustls-webpki `LICENSE` (916 bytes, Copyright 2015 Brian Smith, "AUTHORS") | `5b698ca13897be3afdb7174256fa1574f8c6892b8bea1a66dd6469d3fe27885a` |
| untrusted `LICENSE.txt` (769 bytes, Copyright 2015-2016 Brian Smith, "AUTHORS") | `7abd9b6960dcf7d4d0a48606a5b71bfe37d472db68d70637f3a58a56785f1621` |
| rustls `LICENSE-APACHE` | `a60eea817514531668d7e00765731449fe14d059d3249e0bc93b36de45f759f2` |
| rustls `LICENSE-ISC` (Copyright (c) 2016 Joseph Birr-Pixton) | `7cfafc877eccc46c0e346ccbaa5c51bb6b894d2b818e617d970211e232785ad4` |
| rustls `LICENSE-MIT` | `709e3175b4212f7b13aa93971c9f62ff8c69ec45ad8c6532a7e0c41d7a7d6f8c` |
| whoami `LICENSE_APACHE` | `62c7a1e35f56406896d7aa7ca52d0cc0d272ac022b5d2796e7d6905db8a3636a` |
| whoami `LICENSE_MIT` (permission text, no copyright line) | `508a77d2e7b51d98adeed32648ad124b7b30241a8e70b2e72c99f92d8e5874d1` |
| whoami `LICENSE_BOOST` (Boost Software License 1.0) | `c9bff75738922193e67fa726fa225535870d2aa1059f91452c411736284ad566` |
| wasi `LICENSE-APACHE` | `a60eea817514531668d7e00765731449fe14d059d3249e0bc93b36de45f759f2` |
| wasi `LICENSE-MIT` (no Copyright string) | `23f18e03dc49df91622fe2a76176497404e46ced8a715d9d2b67a7446571cca3` |
| wasi `LICENSE-Apache-2.0_WITH_LLVM-exception` | `268872b9816f90fd8e85db5a28d33f8150ebb8dd016653fb39ef1f94f2686bc5` |
| webpki-roots `LICENSE` | `e271993808fec50ab29350b39539cdec611a9103f827e0aa26d61da70e2d33f8` |
| r-efi 6.0.0 `README.md` | `420f65fec3ba2e89d49da51b222d2881d26959037a7b7d3794143db4bd4627b1` |
| r-efi 6.0.0 `AUTHORS` | `d027e91dbc9cdbb2f1190068e498bd6b61cff022b6a032b191021ba658d96111` |

`ring` has no `NOTICE` file and no `AUTHORS` file. Apache-2.0 conditions 4(a), 4(b), and 4(c) apply to the Apache-covered parts. Condition 4(d) has no NOTICE file to attach. The fiat LICENSE names authors that are not listed in the crate; this record does not invent them. rustls-webpki `LICENSE` mentions `third-party/chromium/LICENSE`; the published crate has no `third-party` directory. Keep that sentence. rustls `LICENSE-APACHE`, ring once_cell `LICENSE-APACHE`, and wasi `LICENSE-APACHE` are the same bytes.

Every copy of an ISC crate keeps that crate's copyright line and permission notice. `ring` also keeps `LICENSE`, `LICENSE-other-bits`, and the whole `LICENSE-BoringSSL`.

## Tool

cargo-deny 0.20.2. Crate sha256 `e528dfcbe739af7ce37a77d3d6df1b29dd6887b1c701d888820c0f16b864f737`, the pin in `crates/xtask/src/supply.rs`. Installed with `cargo install cargo-deny --version 0.20.2 --locked` into temporary `CARGO_HOME` `/tmp/oo-a067-32680b16-6285-466e-a244-7f50e87e2b44/deny-home`. The downloaded `.crate` matched the pin. `cargo-deny --version` printed `cargo-deny 0.20.2`. Host compiler for that install: rustc 1.98.1 `(48a229cea 2026-09-01)`, `/opt/homebrew/bin/cargo`. No other tool version was installed. The binary was a host process, not a second rust-builder container.

Exit code 4 is the license-check bit (`1 << 2` in cargo-deny 0.20.2 `stats.rs`) whenever the license check has at least one error. It is not a count of four errors. JSON diagnostics are on stderr. Human stdout is one summary line. `--format json` left stdout empty; those empty files were not copied.

## MSRV smoke, before the allow edit

Archive of `f61de8d83c3923fee8d98915eac290471841b83f` (tree `968704d558304fc49a1e450db6816363468eecc2`). Snapshot input sha256 in `raw/msrv/archive-inputs.sha256`:

| File | sha256 |
|---|---|
| `ci/deny.toml` | `ee87f177fbf221221c5f3a7f0020dc393a047da9af77ec797c2953fc25136559` |
| `Cargo.toml` | `b18a3b53f594945296a440ceecebaf702bc0fb26605f2c8d4509feafa4ed1a5c` |
| `Cargo.lock` | `636e19b8761bd4f212381fd0abe6a3040ca6ee8024aed06dd350a1fd55ec32d1` |
| `rust-toolchain.toml` | `7553618f800829444096beea3dce84c3d07585efda504c19adb6787369bbd9e8` |

The snapshot `ci/deny.toml` hash is the pre-ISC file, so this smoke ran before the allow edit.

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `--pull never`, `--platform linux/arm64`. Container `oo-a067-msrv-32680b16-6285-466e-a244-7f50e87e2b44`, id `7f54627b399019459c020e4169bbc997c81305157678b541d750738fd27c2510`. Flags: `--init`, `--user 0:0`, `--security-opt no-new-privileges`, `--cap-drop ALL`, `--memory 8g`, `--memory-swap 8g`, `--cpus 4`, `--pids-limit 4096`. Default bridge. Readonly binds of the snapshot and `smoke.sh`. Writable binds of this run's cargo home and target. No host home, no docker socket, no host network, no privileged mode.

Inside the container, `RUSTUP_TOOLCHAIN=1.94.0`. `rustc -Vv` printed `rustc 1.94.0 (4a4ef493e 2026-03-02)`.

| Phase | Command | Exit |
|---|---|---|
| fetch | `cargo fetch --locked` | 0 (`phase_exit=0 phase=cargo_fetch_locked`, console line 166) |
| check | `cargo check -p oo-core --locked --offline` | 0 (`phase_exit=0 phase=cargo_check_oo_core`) |

119 `Downloaded` lines, all before the fetch marker. Zero downloads between the fetch marker and the check marker. `Finished dev profile [unoptimized + debuginfo] target(s) in 11.01s`. `elapsed_seconds=16`. Inspect after exit: `exit=0 status=exited network=bridge privileged=false user=0:0`, image digest matches the pin. This is `cargo check` of `oo-core` on the f61 archive. It is not a test run, not an air-gap claim, and not source registration.

## b2 graph, cargo-deny 0.20.2

Source `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74`, parent `4213134d136ff50bd2f7d422f3f840e48e038b6d`, tree `68fbc1b863e4ae46695bd71b52bb4af396adcdfe`. Read with `git -C /Users/redux/.codex/worktrees/product-pg/open-ontology archive`. After the archive, that checkout still resolved to `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74`. It was not checked out onto another commit. The archive was not copied into this commit.

Working directory: the archive. `CARGO_HOME` was the temporary deny home. `RUSTUP_TOOLCHAIN` unset. Host rustc 1.98.1 Homebrew.

Before, config = archive `ci/deny.toml`:

```
cargo-deny --format json --config ci/deny.toml --locked check
cargo-deny --format human --config ci/deny.toml --locked check
```

Both exits 4. Human stdout: `advisories ok, bans ok, licenses FAILED, sources ok`. stderr `raw/b2-before.stderr`, 48094 bytes. Summary: advisories 0, bans 0 errors and 12 warnings, licenses 4 errors / 241 helps / 0 warnings, sources 0.

| Crate | Version | Code | Spans |
|---|---|---|---|
| ring | 0.17.14 | rejected | `Apache-2.0 AND ISC`, `ISC` |
| rustls-webpki | 0.103.15 | rejected | `ISC`, `ISC` |
| untrusted | 0.9.0 | rejected | `ISC`, `ISC` |
| webpki-roots | 1.0.9 | rejected | `CDLA-Permissive-2.0`, `CDLA-Permissive-2.0` |

Message on each: `failed to satisfy license requirements`. The ISC notes printed by the tool are `OSI approved` and `FSF Free/Libre`. The duplicate warnings are base64, block-buffer, cpufeatures, crypto-common, digest, getrandom, hashbrown, r-efi, sha2, syn, synstructure, windows-sys. Those 12 warnings are unchanged in the after log. The gold diagnostics were not edited.

After, same archive, config = candidate file that differs by the ISC line and is byte-identical to the repo `ci/deny.toml` (`diff -q` during the run):

```
cargo-deny --format json --config <candidate-deny.toml> --locked check
cargo-deny --format human --config <candidate-deny.toml> --locked check
```

Both exits 4. Human stdout is the same one line, `licenses FAILED`. stderr `raw/b2-after.stderr`, 45183 bytes. Summary: advisories 0, bans 0 errors and 12 warnings, licenses 1 error / 244 helps / 0 warnings, sources 0. The only license error is `webpki-roots` 1.0.9, spans `CDLA-Permissive-2.0`, `CDLA-Permissive-2.0`, code `rejected`. Helps moved from 241 to 244. The three ISC crates are absent from the error list.

## f61 archive with the candidate config

Same binary, working directory the f61 archive above, config the repo `ci/deny.toml`:

```
cargo-deny --format json --config <repo ci/deny.toml> --locked check
cargo-deny --format human --config <repo ci/deny.toml> --locked check
```

Both exits 0. Human stdout: `advisories ok, bans ok, licenses ok, sources ok`. Licenses: 0 errors, 135 helps, 1 warning (`license-not-encountered`, span `ISC`). Bans: 0 errors, 1 warning, duplicate `syn` 2.0.119 and `syn` 3.0.6. Advisories 0. Sources 0. stderr 7044 bytes.

That pass is the foundation lock, which has no `ring`. `git grep` of `name = "ring"` in `HEAD:Cargo.lock` during this write also found no match. The f61 exit 0 does not prove the ISC correction. The proof is the b2 before/after error list plus `fixtures/positive-isc`.

## Independent fixtures

Each directory under `contracts/supply-chain/isc/fixtures/` is its own workspace, edition 2021, `publish = false`, no dependencies. Lockfiles were generated with `cargo generate-lockfile --manifest-path <fixture>/Cargo.toml --offline` in an isolated `CARGO_HOME`. Package names are `oo-isc-fixture-<dir>`.

Command, cargo-deny 0.20.2, for each fixture, original config = b2 archive `ci/deny.toml`, candidate config = repo `ci/deny.toml`:

```
cargo-deny --format json --manifest-path contracts/supply-chain/isc/fixtures/<name>/Cargo.toml --config <cfg> --locked check
```

Re-parse of the 22 copied stderr files in this write: every run has advisories errors 0, bans errors 0, sources errors 0, and no advisory, ban, or source warnings. License results:

| Fixture | Expression | Original exit / license errors | Candidate exit / license errors |
|---|---|---|---|
| positive-isc | `ISC`, full synthetic ISC `LICENSE` | 4 / 1 `rejected` span `ISC` | 0 / 0 |
| negative-gpl-only | `GPL-3.0-only` | 4 / 1 span `GPL-3.0-only` | 4 / 1 span `GPL-3.0-only` |
| negative-agpl-only | `AGPL-3.0-only` | 4 / 1 | 4 / 1 |
| negative-sspl-only | `SSPL-1.0` | 4 / 1 | 4 / 1 |
| negative-bsl-only | `BUSL-1.1` | 4 / 1 | 4 / 1 |
| negative-boost-bsl-only | `BSL-1.0` | 4 / 1 span `BSL-1.0` | 4 / 1 span `BSL-1.0` |
| negative-unknown | `LicenseRef-OO-Synthetic-Unknown` | 4 / 1 | 4 / 1 |
| negative-llvm-exception-only | `Apache-2.0 WITH LLVM-exception` | 4 / 1 | 4 / 1 |
| dual-mit-or-gpl | `MIT OR GPL-3.0-only`, `LICENSE-MIT` present | 0 / 0 | 0 / 0 |
| exception-or-apache-mit | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | 0 / 0 | 0 / 0 |
| mit-or-gpl-missing-mit-text | `MIT OR GPL-3.0-only`, neither text shipped | 0 / 0 | 0 / 0 |

Negative-control license warnings are 7 on the original config and 8 on the candidate. The extra warning is unused `ISC` (`license-not-encountered`). `positive-isc` stays at 7 license warnings on the candidate because `ISC` is encountered there. `dual-mit-or-gpl` is 6 then 7. `exception-or-apache-mit` is 5 then 6. Those warnings do not change the exit.

`positive-isc` is the independent proof that adding `ISC` accepts an ISC crate that the old file rejected. The GPL-only, AGPL-only, SSPL-only, BUSL-1.1-only, BSL-1.0-only, unknown, and LLVM-exception-only crates stay rejected on both configs. `Apache-2.0 WITH LLVM-exception` alone stays rejected; the exception id was not allowlisted.

`dual-mit-or-gpl` and `exception-or-apache-mit` pass on the old file and on the new file. That OR behavior is pre-existing. The selected obligation for `dual-mit-or-gpl` is the MIT text in `LICENSE-MIT`. GPL-3.0-only stays rejected when it is the only expression. `mit-or-gpl-missing-mit-text` also exits 0 on both configs, and its `LICENSE` states that neither the MIT text nor the GPL text is present. A deny exit of 0 is the expression decision. It is not proof that the MIT text shipped. `r-efi` 6.0.0 is the real crate with that gap.

`supply-chain/policy.md` §2 says a GPL identifier inside an `OR` stays out of the default artifact. `ci/deny.toml` does not implement that token scan, and this commit does not edit that file. Deny already accepted an allowed side of an `OR` before ISC was added (`whoami`, `r-efi`, `wasi` on the b2 graph).

Negative fixture license files, other than the Boost note and the missing-text note, are one-line declared-SPDX notes. They are not copies of GPL, AGPL, SSPL, or Business Source text. The Boost fixture `LICENSE` says it is not the Boost license text.

## Cleanup

`docker rm -f oo-a067-msrv-32680b16-6285-466e-a244-7f50e87e2b44` exited 0 (`raw/msrv/docker-rm.txt`). At that moment `docker ps` listed `oo-a064-pg1710-jobs` up; this run did not stop it and did not prune. A later `docker ps` in the report-writing turn listed no containers, and `docker ps -a --filter name=oo-a067` listed none. `/tmp/oo-a067-32680b16-6285-466e-a244-7f50e87e2b44` is gone. Crates, the deny binary, and extracted sources went with it. Hashes and the NOTICE quotes were checked before that removal. No customer data, no push, no pull request.

## Scope left outside this commit

Root manifests, locks, core, source crates, harness source, build infra, benches, dependency pins, docs, spec, plan, ledger, `supply-chain/policy.md`, and other sessions' evidence are untouched by this commit. The working tree may contain other agents' edits. Those paths were not staged. a063 remains the root-lock writer. a066 remains the owner of source correction in the existing PostgreSQL checkout. Current b2 deny is still nonzero by design.
