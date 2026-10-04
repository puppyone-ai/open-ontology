# Dependency delta

Compared with `HEAD` before this task (`d61945f1ae66f9d3b35666385cc9d94d5d4ae08a` `Cargo.lock`). `supply/lock-vs-head-removed.txt` is empty. `supply/lock-vs-head-added.txt` lists 24 name/version pairs. No existing name and version was removed or replaced. `cargo update` was not used. The root `Cargo.toml` was not edited. There is no `[workspace.dependencies]` pin.

The first `cargo fetch` inside the install builder produced this delta. Later fetches, after the lock already contained the pairs, added and removed nothing (`supply/lock-added.txt` and `supply/lock-removed.txt` from the green run are empty). The first copies are `supply/lock-added-resolve1.txt`, `supply/lock-removed-resolve1.txt`, and `linux/attempt1-clippy/supply/`.

Direct gateway dependencies, all `=` pins, default features off:

| Crate | Version | crates.io created_at | License field | MSRV | Lock checksum |
| --- | --- | --- | --- | --- | --- |
| tokio-postgres | 0.7.18 | 2026-06-12 | MIT OR Apache-2.0 | 1.85 | `a528f7d280f6d5b9cd149635c8705b0dd049754bc67d81d31fa25169a93809d3` |
| postgres-types | 0.2.14 | 2026-06-12 | MIT OR Apache-2.0 | 1.85 | `851ca9db4932932d69f3ea811b1abe63087a0f740a47692619dd40d4899b68be` |
| postgres-protocol | 0.6.12 | 2026-06-12 | MIT OR Apache-2.0 | 1.85 | `08808e3c483c46e999108051c78334f473d5adb59d78bb80a1268c7e6aa6c514` |
| tokio-postgres-rustls | 0.14.0 | 2026-05-21 | MIT | none published | `4c2ad44aa0ae96db89c4742212ed41645b2f597311ff6e1945542a4d9fadc2fb` |

`tokio` 1.53.1, `rustls` 0.23.45, and `tokio-rustls` 0.26.5 were already locked. Their checksums are unchanged. Features enabled on the gateway: tokio `io-util`, `macros`, `net`, `rt-multi-thread`, `sync`, `time`; rustls `std`, `tls12`, `aws-lc-rs`; tokio-rustls `tls12`, `aws-lc-rs`; tokio-postgres `runtime`; tokio-postgres-rustls `aws-lc-rs`. The dev dependency `sha2` 0.10.9 was already locked. `postgres-types` has no `with-*` features. `deadpool-postgres` is not a dependency. The sync crate `postgres` is not a dependency. SQLx is not a gateway dependency. `oo-pipeline` is not a gateway dependency.

`supply/crates-io-created.tsv` is the crates.io API row for every new pair. Every `created_at` is before 2026-09-28. The newest is `libredox` 0.1.25 at 2026-09-21. None of those versions were yanked. Lock checksums match the API checksums recorded in that file.

Edges that appeared without a version change:

- `digest` 0.11.3 checksum `f1dd6dbb5841937940781866fa1281a1ff7bd3bf827091440879f9994983d5c2` is unchanged. The lock now lists `const-oid` 0.10.2 under it. `hmac` 0.13.0, `md-5` 0.11.0, and `sha2` 0.11.0 already depended on `digest` 0.11.3.
- `whoami` 2.1.3 checksum `626c4bac6755d76ffc12cb01b2eac751db1996b9e0041de9aa02c8c211ddc82c` is unchanged. The lock now lists `libc`, `libredox`, `objc2-system-configuration`, `wasite`, and `web-sys`. `tokio-postgres` 0.7.18 depends on `whoami`. An existing package in the lock already depended on `whoami` as well.
- `x509-cert` 0.2.5 depends on `const-oid` 0.9.6, `der` 0.7.10, `spki` 0.7.3, and `tls_codec` 0.4.2. That is a separate `const-oid` line from 0.10.2.
- `phf` 0.13.1 is a second `phf` line. `phf` 0.12.1 checksum is unchanged.

License fields that need a reviewer's eye, because this task did not run `cargo deny` or `cargo audit`:

- `fallible-iterator` 0.2.0 publishes `MIT/Apache-2.0` with a slash.
- `wasi` 0.14.7+wasi-0.2.4 publishes `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`.
- `wasite` 1.0.2 publishes `Apache-2.0 OR BSL-1.0 OR MIT`.

`ci/deny.toml` allows Apache-2.0, MIT, ISC, BSD-3-Clause, Unicode-3.0, Zlib, Unlicense, and MIT-0. A passing install test is not a supply-chain release approval and is not an SBOM.

`is_gateway_postgres_stack` names `tokio-postgres`, `postgres-types`, `postgres-protocol`, and `deadpool-postgres`. `tokio-postgres-rustls` is not in that list. The gate's own comment says a name missing from the taxonomy is not approval to link it. The three focused architecture tests passed on this workspace after the fetch. The gate was not edited.
