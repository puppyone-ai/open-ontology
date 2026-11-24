# B-S4-01: supply-chain record for the s3 transport (no new package)

`crates/oo-connectors/Cargo.toml` (feature `objectstore-s3`): `["objectstore", "dep:opendal", "opendal/services-s3",
"dep:reqwest", "dep:futures-util", "dep:http"]`, and a new optional direct dependency `http = "=1.5.0"`.

Cargo.lock: exactly one line (patches/B-S4-01-cargo-lock.patch): `oo-connectors` gains the edge `"http"`.
New locked packages: **none**. `http 1.5.0`, `reqwest 0.13.5` and `futures-util 0.3.34` were already locked
(opendal-core uses http; reqwest and futures-util by the `opensearch` and `mysql-binlog` features).

Decision for the `opendal-audit.json` note: the transport is our own `HttpTransport` on reqwest
(`crates/oo-connectors/src/objectstore/opendal.rs`), and the retry (200 ms doubling, 10 s cap, 4 attempts) is in that file.
So `backon 1.6.0`, `gloo-timers 0.3.0`, `opendal-http-transport-reqwest 0.59.5`, `opendal-layer-retry 0.59.5` and
`wasm-streams 0.5.0` stay out of the lock. Without the `executors-tokio` feature ranged reads, listing and stat work (tested on
RustFS and MinIO); OpenDAL's multipart writer (tests only) also works without it.
Images (not crates): rustfs/rustfs 1.0.6 `sha256:ee717072290bec9b11abfc0365f8310b00df1cd9b3639943edae47bdef05044a`
(Apache-2.0) and pgsty/minio RELEASE.2026-08-04T00-00-00Z `sha256:b6bfe7239bfc83fb90d31612d9704d86039dd714f7904b3f1ad68f211e602372`
(AGPL-3.0 drop-in fork; test container only, never shipped).
