# ISC provenance

Bytes below were checked two ways. A crate file was downloaded from `https://static.crates.io/crates/<name>/<name>-<version>.crate` and its sha256 was compared with `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74:Cargo.lock`. cargo-deny 0.20.2 then scanned the same versions from its temporary registry cache while checking that commit. Publish times and the yanked flag are the crates.io version API (`created_at`, `yanked`) read on 2026-10-02. None of these versions were yanked.

The source commit is `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74`, parent `4213134d136ff50bd2f7d422f3f840e48e038b6d`, tree `68fbc1b863e4ae46695bd71b52bb4af396adcdfe`. The checkout at `/Users/redux/.codex/worktrees/product-pg/open-ontology` was not moved. The archive used for deny was `git archive` of that commit.

| Crate | Version | Crate sha256 | Published | SPDX on the crate |
|---|---|---|---|---|
| ring | 0.17.14 | `a4689e6c2294d81e88dc6261c768b63bc4fcdb852be6d1352498b114f61383b7` | 2025-03-11T19:42:03.889273Z | `Apache-2.0 AND ISC` |
| rustls-webpki | 0.103.15 | `f3c3cf1d8b1e7d4927e2d154c3fcb02979afb9939629c62cd9048d4f07b60ac2` | 2026-08-21T16:40:00.687282Z | `ISC` |
| untrusted | 0.9.0 | `8ecb6da28b8a351d773b68d5825ac39017e680750f980f3a1a85cd8dd28a47c1` | 2021-07-13T22:13:26.650010Z | `ISC` |
| rustls | 0.23.45 | `0d41d731c7d2f962d1ccc364cec258de3c0e93b38c2fb3ba97ac74513048d634` | 2026-09-14T15:11:17.808465Z | `Apache-2.0 OR ISC OR MIT` |
| webpki-roots | 1.0.9 | `7dcd9d09a39985f5344844e66b0c530a33843579125f23e21e9f0f220850f22a` | 2026-07-18T09:30:44.802479Z | `CDLA-Permissive-2.0` |
| whoami | 2.1.3 | `626c4bac6755d76ffc12cb01b2eac751db1996b9e0041de9aa02c8c211ddc82c` | not re-fetched as an API row for this table | `Apache-2.0 OR BSL-1.0 OR MIT` |
| wasi | 0.11.1+wasi-snapshot-preview1 | `ccf3ec651a847eb01de73ccad15eb7d99f80485de043efb2f370cd654f4ea44b` | not re-fetched as an API row for this table | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` |
| r-efi | 6.0.0 | `f8dcc9c7d52a811697d2151c701e0d08956f92b0e24136cf4cf27b57a6a0d9bf` | 2026-02-03T09:53:46.251122Z | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` |

`whoami` and `wasi` publish times were not requested from the API in the a067 run. Their checksums match the lock, and the license files below were read from the cargo-deny cache of that lock. `r-efi` 5.3.0 (`69cdb34c158ceb288df11e18b4bd39de994f6657d83847bdffdbd7f346754b0f`, published 2025-06-17T09:12:38.227277Z) is in that lock and in the current root `Cargo.lock`, with the same SPDX expression. a067 did not open its files. The correction in [ERRATUM.md](ERRATUM.md) downloaded `r-efi` 6.0.0, `r-efi` 5.3.0, and `rustls` 0.23.45 from `https://static.crates.io` and opened both r-efi `AUTHORS` files, both r-efi `README.md` files, and rustls `LICENSE-ISC`. The r-efi crate sha256 values match the crates.io API `checksum` and `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74:Cargo.lock`. The rustls crate sha256 matches the row above. None of these three versions was yanked. Their API `created_at` values match the publish times in this table.

## License-file sha256

| Path in the crate | sha256 |
|---|---|
| ring `LICENSE` | `b3d734001a94efff3579978d953391aa7115f877657d25eb54037a43875d078a` |
| ring `LICENSE-BoringSSL` | `005fc765ddc5115da796cca915baa9557abae13ff35e0a47c47affc56f6c414d` |
| ring `LICENSE-other-bits` | `f025ccfb7dfb6bdfedc75ca0f67acc69e6fb4998143d834f7c2f38a29989680f` |
| ring `src/polyfill/once_cell/LICENSE-APACHE` | `a60eea817514531668d7e00765731449fe14d059d3249e0bc93b36de45f759f2` |
| ring `src/polyfill/once_cell/LICENSE-MIT` | `6ee2ed6c77710de911761acd5fc1ad1da00f476beb1a7ef27e78c2d1858deafc` |
| ring `third_party/fiat/LICENSE` | `9eacbcb81be660840c714a560a9d65ba07913db98dd4baf969f78dd499fdd60f` |
| rustls-webpki `LICENSE` | `5b698ca13897be3afdb7174256fa1574f8c6892b8bea1a66dd6469d3fe27885a` |
| untrusted `LICENSE.txt` | `7abd9b6960dcf7d4d0a48606a5b71bfe37d472db68d70637f3a58a56785f1621` |
| rustls `LICENSE-APACHE` | `a60eea817514531668d7e00765731449fe14d059d3249e0bc93b36de45f759f2` |
| rustls `LICENSE-ISC` | `7cfafc877eccc46c0e346ccbaa5c51bb6b894d2b818e617d970211e232785ad4` |
| rustls `LICENSE-MIT` | `709e3175b4212f7b13aa93971c9f62ff8c69ec45ad8c6532a7e0c41d7a7d6f8c` |
| whoami `LICENSE_APACHE` | `62c7a1e35f56406896d7aa7ca52d0cc0d272ac022b5d2796e7d6905db8a3636a` |
| whoami `LICENSE_BOOST` | `c9bff75738922193e67fa726fa225535870d2aa1059f91452c411736284ad566` |
| whoami `LICENSE_MIT` | `508a77d2e7b51d98adeed32648ad124b7b30241a8e70b2e72c99f92d8e5874d1` |
| wasi `LICENSE-APACHE` | `a60eea817514531668d7e00765731449fe14d059d3249e0bc93b36de45f759f2` |
| wasi `LICENSE-Apache-2.0_WITH_LLVM-exception` | `268872b9816f90fd8e85db5a28d33f8150ebb8dd016653fb39ef1f94f2686bc5` |
| wasi `LICENSE-MIT` | `23f18e03dc49df91622fe2a76176497404e46ced8a715d9d2b67a7446571cca3` |
| webpki-roots `LICENSE` | `e271993808fec50ab29350b39539cdec611a9103f827e0aa26d61da70e2d33f8` |
| r-efi 6.0.0 `README.md` | `420f65fec3ba2e89d49da51b222d2881d26959037a7b7d3794143db4bd4627b1` |
| r-efi 6.0.0 `AUTHORS` | `d027e91dbc9cdbb2f1190068e498bd6b61cff022b6a032b191021ba658d96111` |
| r-efi 5.3.0 `README.md` | `420f65fec3ba2e89d49da51b222d2881d26959037a7b7d3794143db4bd4627b1` |
| r-efi 5.3.0 `AUTHORS` | `ff92bed461f50338dd703a9ba9aee496a425957873df1d91192776e4bdf5dda7` |

`ring` has no `NOTICE` file and no `AUTHORS` file. `third_party/fiat/LICENSE` names an AUTHORS file that is not in the crate. `rustls-webpki` 0.103.15 has no `third-party` directory. Its `LICENSE` still contains the sentence that points at `third-party/chromium/LICENSE`. `r-efi` 6.0.0 and 5.3.0 have no file whose name is `LICENSE`, `COPYING`, or `NOTICE`. Each `README.md` states the SPDX expression as markdown and does not contain the MIT grant. Each `AUTHORS` file contains that grant under `AUTHORS-MIT`. Byte copies of `r-efi` 6.0.0 `AUTHORS`, `r-efi` 5.3.0 `AUTHORS`, and rustls 0.23.45 `LICENSE-ISC` are under [bodies/](bodies/). Their sha256 values are the rows above, including rustls `LICENSE-ISC` `7cfafc877eccc46c0e346ccbaa5c51bb6b894d2b818e617d970211e232785ad4`. The second line of that ISC file is `Copyright (c) 2016, Joseph Birr-Pixton <jpixton@gmail.com>`.

rustls `LICENSE-APACHE`, ring `once_cell/LICENSE-APACHE`, and wasi `LICENSE-APACHE` are the same sha256.

## Tool

cargo-deny 0.20.2, crate sha256 `e528dfcbe739af7ce37a77d3d6df1b29dd6887b1c701d888820c0f16b864f737`, the pin in `crates/xtask/src/supply.rs`. It was installed with `cargo install cargo-deny --version 0.20.2 --locked` into a temporary `CARGO_HOME`. The downloaded `.crate` matched that pin. The host compiler for the install was rustc 1.98.1 `(48a229cea 2026-09-01)`, Homebrew. No other tool version was installed.
