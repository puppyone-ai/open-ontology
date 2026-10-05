# Dependency delta

Source commit `755fb84acc7f80e5ba2fba1c2b80412dab7fab26`. Parent `72b5136ba23d134916425132523383947046aeff`.

`cargo fetch` on Linux rustc 1.94.0 locked five packages. Every package version that was already in the parent lock kept its checksum. No existing version was removed. `cargo deny` is not installed here and was not run. `ci/deny.toml` warns on multiple versions and allows MIT and Apache-2.0.

Direct pins in `crates/oo-control/Cargo.toml`, default features off:

| Crate | Version | Features |
|---|---|---|
| object_store | =0.14.2 | fs |
| sha2 | =0.10.9 | std |
| getrandom | =0.2.17 | std |
| futures-util | =0.3.34 | std |
| tokio (dev) | =1.53.1 | macros, rt-multi-thread |

sha2 0.10.9, getrandom 0.2.17, futures-util 0.3.34, and tokio 1.53.1 were already in the parent lock. This slice does not bump them. `oo-core` and `oo-jobs` stay path dependencies.

## Added to the lock

Checksums are the crates.io `.crate` digests. The file sha256 matched the lock checksum for each row. Publish times are `created_at` from `https://crates.io/api/v1/crates/<name>/<version>`. This project relies on Apache-2.0 where the license is MIT OR Apache-2.0, and on MIT for nix.

| Crate | Version | Published | MSRV | License relied on | Checksum |
|---|---|---|---|---|---|
| object_store | 0.14.2 | 2026-09-15T15:15:46Z | 1.85 | Apache-2.0 | f1796bc93603f78c5760a69f2d58badc9618d22adade0a95385bb2adbae4eb94 |
| nix | 0.31.3 | 2026-05-11T07:01:18Z | 1.69 | MIT | cf20d2fde8ff38632c426f1165ed7436270b44f199fc55284c38276f9db47c3d |
| humantime | 2.4.0 | 2026-07-02T11:38:57Z | 1.60 | Apache-2.0 | 15cdd26707701c53297e2fa6afb323d55fbc1d0810c3aec078ae3ef0424c3c15 |
| futures-macro | 0.3.34 | 2026-08-11T12:13:14Z | 1.71 | Apache-2.0 | 9fb9654ba8355388abeb8dcb4fc62f511300867002afc858860463bdd9fe0c44 |
| itertools | 0.15.0 | 2026-06-16T15:09:28Z | 1.63.0 | Apache-2.0 | 8b4baf93f58d4425749ca49a51c50ebab072c5df6994d08fed93541c331481dc |

`itertools` 0.15.0 sits beside the itertools versions already in the lock. The older rows were not rewritten. `object_store` feature `fs` also names `walkdir`, `tokio`, and `windows-sys`. Those crates were already locked. `cargo tree -p oo-control` shows `object_store v0.14.2 fs,nix,tokio,walkdir,windows-sys`. It does not show `aws`, `gcp`, `azure`, or `http`.

`nix` 0.31.3 is built with object_store's `default-features = false` and no extra nix features. `oo-control` compiled and the file-hosting tests ran on `aarch64-unknown-linux-gnu`, so `nix::unistd::close` resolved in that configuration.

Product `rust-version` remains 1.94.0. The highest `rust-version` among the added crates is object_store 1.85.
