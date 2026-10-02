# Helper supply tools

The fixed helper graph was scanned with cargo-audit 0.22.2 and cargo-deny 0.20.2 on Linux arm64, Rust 1.94.0. Both tools exited 0. cargo-audit reported 0 vulnerabilities and an empty warning object. cargo-deny reported 0 advisory, ban, and source errors. The license check exited 0 with 6 warnings for allow-list entries this graph does not use. Those allows were left in place.

That tool result does not admit the helper crates, and it does not close SP00, the SDK, P5, the root supply graph, signatures, secret rotation, or full AT-SEC-05. `audited` stays false. `full_at_sec_05` stays false.

## Fixed identity

| Field | Value |
| --- | --- |
| Scanned commit | `f1a6005971e3dc3075094c87ed670fc9d25e9ae0` |
| Tree | `86c64f512d67da47ae379a53e34342de8e317f25` |
| Parent | `7143b2d9a7888d242983e5b9c10ef597246d5848` |
| Archive sha256 | `97d4218c736fc2eacb23f01d65e0d7b901049743002e2786870a45f3352efa04` |
| Helper lock sha256 | `14a0a0c66d604bae81f59b770e66a2d7cf00b38e427c3afbc8344bb4864189c1` |
| `ci/deny.toml` sha256 | `4320807bfe747fe339ce7bf539a1cff5604e8e0c7677760e2cbc7abc1868624b` |

The scan input was `git archive` of that commit. At launch the workspace HEAD was `77abcc1731a91cb9933847133e7ae354d289fd46`, and it was not the scan input. The container mounted the archive read-only. No product source, manifest, lock, policy, or deny config was edited. Nothing was staged or committed.

## Runtime

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `--pull never`, platform `linux/arm64`. Inside: Debian 13 (trixie), `Linux 6.12.76-linuxkit aarch64`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`, `cargo 1.94.0 (85eff7c80 2026-01-15)`, host `aarch64-unknown-linux-gnu`. `RUSTUP_TOOLCHAIN=1.94.0` overrode `rust-toolchain.toml` channel `1.98.1`. The same `rustc 1.94.0` string was recorded before install and after the scans.

The container was not privileged. CapDrop was `ALL`, and `/proc/1/status` showed every capability set as zero. `no-new-privileges` was set. Memory and memory-swap were 8 GiB, CPUs 4, pids limit 4096. The network was a task bridge, not host networking. `HOME` inside the container was `/work/home`. The host home and host cargo directories were not mounted. This run used the network for crates.io and for `https://github.com/RustSec/advisory-db.git`. It was not an air-gapped or offline scan.

Run id `2a066bb3-3ca8-4665-9879-24a8b07b6053`. Container `oo-a101-helper-supply-2a066bb3-3ca8-4665-9879-24a8b07b6053` (`915a3ebd8ac1`), exit 0, OOMKilled false. Before the container, host `cargo`/`rustc`/`rustup` count was 0 and no Rust image container was running. Before install, the only Rust image container was this one.

## Helper graph

`cargo metadata --locked` and `cargo tree -e features` on `testing/harness/containment-supervisor/Cargo.toml` resolved six packages. nix is enabled with `signal`, and `signal` enables `process`. Default features of nix are empty. `--all-features` was not passed. cargo-deny `--locked` exited 0 against that lock, so the resolve did not need crates outside it.

| Package | Version | Checksum | Features |
| --- | --- | --- | --- |
| containment-supervisor | 0.1.0 | path package | none |
| nix | 0.31.3 | `cf20d2fde8ff38632c426f1165ed7436270b44f199fc55284c38276f9db47c3d` | `process`, `signal` |
| libc | 0.2.189 | `3eaf3ede3fee6db1a4c2ee091bf8a8b4dccdc6d17f656fb07896ee72867612f2` | `default`, `extra_traits`, `std` |
| bitflags | 2.13.2 | `3ded4057c258ba199e2d26386d3af3780957ecaee6c4ef4041c6b4b8b97c0b06` | `default` |
| cfg-if | 1.0.5 | `4e7648175b45a9a48536d676f68d918270699102aa8dab5496df06904c914600` | `default` |
| cfg_aliases | 0.2.2 | `f079e83a288787bcd14a6aea84cee5c87a67c5a3e660c30f557a3d24761b3527` | `default`, build-dependency |

Each registry `.crate` sha256 matched the lock checksum. cargo-audit reported `lockfile.dependency-count` 6.

## Tools

No linux/arm64 cargo-audit or cargo-deny binary was used from a previously recorded path, version, and hash. Both were installed with `cargo install --version <pin> --locked --force` into the task volume. The installed `.crate` bytes matched the harness pins. Official `rust-version` is 1.88 for cargo-audit and 1.88.0 for cargo-deny, both below 1.94.0, so the toolchain was not raised.

| Tool | Crate sha256 | Binary sha256 | ELF |
| --- | --- | --- | --- |
| cargo-audit 0.22.2 | `700c2b240f7fd330c24b675fe429f73a5b676531fcc6300400b2b67f155ba12a` | `53951cddfc4b43822ef9f71b2a32dd8725a54ee493549a8293241d9d9795eead` | class 2, machine 183 (aarch64) |
| cargo-deny 0.20.2 | `e528dfcbe739af7ce37a77d3d6df1b29dd6887b1c701d888820c0f16b864f737` | `44296adb80fd7d3a6ce8737e2806cca409a96757798b399f32f1b535df74884c` | class 2, machine 183 (aarch64) |

Published `2026-06-05T12:33:04.440539Z` and `2026-07-09T17:58:09.303885Z`, both HTTP 200 and unyanked. License files in the tool crates match `supply-chain/licenses/harness/` byte for byte (Apache `a60eea81…` and MIT `29662666…` for cargo-audit; Apache `8173d5c2…` and MIT `090a294a…` for cargo-deny). The tool locks contain 399 packages (cargo-audit) and 211 packages (cargo-deny). That graph is tooling only. Installing it does not approve the helper graph.

Flags used below were present in the pinned `--help` text before the scans ran.

## Commands and exits

| Phase | Exit |
| --- | --- |
| identity | 0 |
| `cargo fetch --locked --manifest-path` helper | 0 |
| `cargo metadata --format-version 1 --locked` | 0 |
| `cargo tree --locked` | 0 |
| `cargo tree --locked -e features` | 0 |
| `cargo install cargo-audit --version 0.22.2 --locked --force` | 0 |
| `cargo install cargo-deny --version 0.20.2 --locked --force` | 0 |
| `cargo-audit audit --json --file` helper `Cargo.lock` | 0 |
| `cargo-deny --format json --config ci/deny.toml --locked --manifest-path` helper `check` | 0 |
| `check advisories` | 0 |
| `check licenses` | 0 |
| `check bans` | 0 |
| `check sources` | 0 |

cargo-audit wrote 373 bytes of JSON to stdout and 0 bytes to stderr. cargo-deny wrote 0 bytes to stdout. Its machine JSON, including the summary, is on stderr (`raw/scan-deny*.err`). `ci/deny.toml` was not modified.

## Advisory database

cargo-audit loaded a database it cloned into the empty task `CARGO_HOME` at `/cargo-home/advisory-db`, remote `https://github.com/RustSec/advisory-db.git`.

| Field | Value |
| --- | --- |
| Commit | `117edb3bed98e9be112f277b7615eea3252e7c43` |
| Commit time | `2026-10-02T10:58:33+02:00` |
| Recorded at | `2026-10-02T12:12:14Z` |
| Advisory count | 1280 |
| Worktree | clean (`porcelain_count=0`) |

That commit is the same commit a096 recorded from the GitHub commits API earlier the same day. a096 did not run cargo-audit or cargo-deny. This clone is the database the tools actually evaluated. The earlier per-crate HTTP checks are not this result. cargo-deny then used the same checkout; the commit did not change between the two records.

cargo-audit JSON:

- `vulnerabilities.found` false, `count` 0, `list` empty
- `warnings` empty
- `settings.informational_warnings`: `unmaintained`, `unsound`, `notice`
- `settings.ignore` empty
- `--no-yanked` was not passed, so yanked crates were in scope
- `--deny` was not passed, so an informational warning would not by itself make the exit nonzero. This run had no such warning.

`RUSTSEC-2021-0119` is present in that database (`raw/RUSTSEC-2021-0119.after-audit.md`, sha256 `b88110013c261da320d45368672e74edd40b6430ca338c244b6f43a454beb5b6`). Its patched set includes `>= 0.23.0`. nix 0.31.3 sits in that range. The tool did not list the advisory. The empty vulnerability list is the tool result; the patched line is the text in the database the tool loaded.

cargo-deny advisories: errors 0, warnings 0. `unmaintained = "all"` and `yanked = "deny"` were the config that ran.

## Age and checksums

Age is the crates.io version object's `created_at`, the first `created_at` in `https://crates.io/api/v1/crates/<name>/<version>`. HTTP status was 200 for all five. The floor is 604800 seconds at `2026-10-02T12:08:28Z`. The stored elapsed time drops the fractional second and then subtracts one second. Every selected version still clears the floor. None are yanked. Each API checksum matches the lock and the downloaded `.crate`.

| Crate | Version `created_at` | Elapsed seconds (trunc) | Crate-object `created_at` (not used) |
| --- | --- | --- | --- |
| nix 0.31.3 | `2026-05-11T07:01:18.650221Z` | 12460030 | `2014-11-11T22:15:48.567559Z` |
| libc 0.2.189 | `2026-07-21T21:33:28.018110Z` | 6273300 | `2015-01-15T20:22:13.100871Z` |
| bitflags 2.13.2 | `2026-09-10T01:11:38.500809Z` | 1940210 | `2015-01-15T08:35:24.901276Z` |
| cfg-if 1.0.5 | `2026-09-16T17:36:11.278420Z` | 1362737 | `2015-07-08T01:11:24.485040Z` |
| cfg_aliases 0.2.2 | `2026-07-16T14:57:16.699789Z` | 6729072 | `2020-04-05T15:56:29.360016Z` |

a082 `supply-resolved.json` stored nix `created_at` `2021-12-30T16:00:11Z`. That timestamp is the second `created_at` in the version payload, the publisher account. The crate object for nix is `2014-11-11T22:15:48.567559Z`. Neither is the selected version time. The age above uses `2026-05-11T07:01:18.650221Z`.

`raw/crate-checksums.txt` also contains one `MISSING_CRATE # lock` line because the comment header of `lock-packages.txt` was read as a package name. The five registry rows above it match.

## License texts and obligations

Upstream license-like files were extracted from the checksum-matched crates. `cmp` against `testing/harness/containment-supervisor/provenance/licenses/` was identical for every helper license and notice file. No declared license file is missing from the crate.

| Package | Expression | Upstream files | Named copyright |
| --- | --- | --- | --- |
| nix 0.31.3 | MIT | `LICENSE` | Copyright (c) 2015 Carl Lerche + nix-rust Authors |
| libc 0.2.189 | MIT OR Apache-2.0 | `LICENSE-MIT`, `LICENSE-APACHE` | Copyright (c) The Rust Project Developers, in the MIT file |
| bitflags 2.13.2 | MIT OR Apache-2.0 | `LICENSE-MIT`, `LICENSE-APACHE` | Copyright (c) 2014 The Rust Project Developers, in the MIT file |
| cfg-if 1.0.5 | MIT OR Apache-2.0 | `LICENSE-MIT`, `LICENSE-APACHE` | Copyright (c) 2014 Alex Crichton, in the MIT file |
| cfg_aliases 0.2.2 | MIT | `LICENSE`, `NOTICES.md` | Copyright (c) 2020 Katharos Technology |

The MIT texts require the copyright notice and the permission notice to stay with copies and substantial portions.

The Apache-2.0 texts require a copy of the license, change notices for modified files, retention of any NOTICE file the work ships, and the patent terms in that license. libc, bitflags, and cfg-if do not ship a `NOTICE` file. Their Apache appendix line is the standard placeholder `Copyright [yyyy] [name of copyright owner]`. The owner notice that is filled in is the MIT file. These crates were not modified here.

cfg_aliases `NOTICES.md` is in the crate. It is the MIT notice for `tectonic_cfg_support::target_cfg` code used by the macro, and the provenance copy matches it. That notice has to be retained with the Katharos MIT notice. The upstream filename is `NOTICES.md`. There is no separate missing license file.

bitflags, libc, and cfg-if are `MIT OR Apache-2.0`. Both license files are in the crate, so the evidence covers both sides of the expression. No audit record names `dual_license_choice` for any of them. cargo-deny accepts the expression because both identifiers are on the allow list. That acceptance does not choose the license this project relies on, and it does not satisfy the record fields in `supply-chain/policy.md`. Policy admission for these five crates is not made.

r-efi is outside this graph and was not opened.

## Warnings

cargo-deny `check licenses`, with `unused-allowed-license = "warn"`, emitted `license-not-encountered` for `BSD-3-Clause`, `ISC`, `MIT-0`, `Unicode-3.0`, `Unlicense`, and `Zlib` (`ci/deny.toml` lines 16–21). Apache-2.0 and MIT were encountered. ISC is allowed and is not a license of this helper graph. The allow list was not edited to clear the warnings. Exit remained 0. Errors were 0 for advisories, licenses, bans, and sources. Bans and sources had 0 warnings. Unknown registry, unknown git, and a new license id did not appear.

## Open gates

- No dependency audit record was written. `audited` is false.
- `dual_license_choice` is unset for the three `OR` crates.
- Root supply, including CDLA, dunce, aws-lc, native code, and image/OCI scans, was not run.
- JavaScript trees, OS image scan, signatures, secret rotation, and full AT-SEC-05 were not run.
- SP00, SDK, and P5 are outside this result.
- The security owner is not appointed. No seven-day exception was created.
- Advisory database commit `117edb3b` is the fetched HEAD at `2026-10-02T12:12:14Z`. It is the same commit a096 already saw. A later database commit would be a new scan.

## Logs, hashes, cleanup

Raw logs, help text, metadata, age JSON, upstream license extracts, and tool provenance locks are under `raw/`. `HASHES.sha256` lists this evidence directory after the report was written.

Removed, by exact id: container `oo-a101-helper-supply-2a066bb3-3ca8-4665-9879-24a8b07b6053`, volumes `oo-a101-2a066bb3-3ca8-4665-9879-24a8b07b6053-cargo` and `oo-a101-2a066bb3-3ca8-4665-9879-24a8b07b6053-target`, network `oo-a101-2a066bb3-3ca8-4665-9879-24a8b07b6053-net`, and `/tmp/oo-a101-2a066bb3-3ca8-4665-9879-24a8b07b6053`. A label filter for this run id was empty afterward. `oo-a100-msrv-5dd178d8` was running on the same Rust image after that removal and was left running. No prune. Process inventories with pid, ppid, etime, and comm are `raw/processes-before.txt`, `raw/processes-before-install.txt`, and `raw/processes-after.txt`.
