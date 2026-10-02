# a073 handoff

Root lock handoff for the coordinator. Source integration of `oo-sdk` stays queued until the independent fixed-SHA review and the coordinator evidence review. Do not treat this as feature delivery.

## Commit

Delivery parent: `d2ea6351414303ad1926fbeb99010607db8d2833`

Delivery commit: recorded below after `git rev-parse`, in the report update commit.

```
DELIVERY_COMMIT
DELIVERY_TREE
```

The report update commit's parent is that delivery commit. Its own hash is whatever `git rev-parse HEAD` prints after the update commit. A commit cannot store its own hash.

## Lock

`Cargo.lock` is the a073 lock delta on the delivery parent. 131 registry versions added, 0 removed. No existing sole version was replaced. Extra versions only: `base64` 0.22.1, `cpufeatures` 0.3.1, `getrandom` 0.2.17 and 0.4.3, `r-efi` 6.0.0. `cargo generate-lockfile` was not used.

Direct new pins: reqwest 0.13.5 (`rustls`, `http2`, default features off), tokio 1.53.1 (`time` only on the library), hyper-util 0.1.20, tokio-rustls 0.26.5, rustls-platform-verifier 0.7.0, cc 1.4.7, find-msvc-tools 0.1.13, yoke-derive 0.8.2. Dev: rustls 0.23.45, socket2 0.6.5. `serde` 1.0.229 and `serde_json` 1.0.151 were already locked.

Not admitted: `CDLA-Permissive-2.0` on `webpki-root-certs` 1.0.9 (locked, not in the host or linux/arm64 selected trees, present under `cargo tree --target all`). `quinn` and `ring` are locked and not in those selected trees. `openssl-probe` on linux/arm64 is the platform certificate probe, not an OpenSSL TLS backend. cargo-audit and cargo-deny were not run. wasm32 was not built.

## Client

`oo-sdk` is a production-shaped HTTP client for `POST /v1/query` JSON only. It uses the frozen `oo-core` door. It does not speak SQL to Postgres and does not cache results. Retry is only the `RATE_LIMITED` / `OVERLOADED` subset. Loopback tests are a protocol oracle.

## Evidence

`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a073/SP09/sdk-json-query/`

- `REPORT.md` commands, exits, provenance, gaps
- `pre-smoke/` Linux 1.94 smoke before the client, exit 0
- `linux-post/` Linux 1.94 focused fetch, test, clippy, all exits 0, run `4ddb03eb-78b6-42ee-b66d-1cc7ea761226`
- `host/` preserved failed tests and the green rerun
- `provenance/crates.json` crates.io audit
- `cleanup.txt` own docker and `/tmp` snapshot removal, exits 0

a073 containers, networks, and volumes are gone. Foreign containers were left in place.
