# Source inventory

Fixed base used for the query-pg archive: `7a6f7eac8b8041eae215c087d1a80930437871ce`.

## Product crate

| Path | Role |
| --- | --- |
| `tools/selfbuild-suggestions/CONTRACT.md` | Frozen contract, written before the Rust. |
| `tools/selfbuild-suggestions/Cargo.toml` | Standalone workspace, rust-version 1.94.0, pinned serde 1.0.229, serde_json 1.0.151, sha2 0.10.9, forbid unsafe. |
| `tools/selfbuild-suggestions/Cargo.lock` | Hand subset of the root lock. Not generated in this session. |
| `tools/selfbuild-suggestions/src/lib.rs` | Crate root. |
| `tools/selfbuild-suggestions/src/main.rs` | CLI `produce` and `produce --config`. |
| `tools/selfbuild-suggestions/src/json.rs` | Duplicate-key parser, canonical JSON, candidate ids. |
| `tools/selfbuild-suggestions/src/produce.rs` | S6, S7, S8, S9, seals, dedup, effects. |
| `tools/selfbuild-suggestions/src/gateway.rs` | `ProcessRunner` spawn of `query-pg`. |
| `tools/selfbuild-suggestions/tests/producer.rs` | Gold, provenance, dedup, stale, cross-project, effects, CLI, fake gateway, ignored live test. |

## Reused recipes, read-only

| Origin | Use |
| --- | --- |
| `testing/real-db/gateway-tls/linux/proof/launch.sh` | Capacity check and docker run flags. |
| `testing/real-db/gateway-tls/pg/pg-wrap.sh` | Postgres entrypoint, mounted read-only. |
| `testing/real-db/gateway-tls/pg/up.sh` | Postgres capability set and resource limits, copied into `pg/up.sh` with a355 names. |
| `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/linux/smoke-2/extract-libclang.sh` | Unchanged libclang extract. SHA-256 `1c2f68d652f667eee5d861ec11bb0ee5654ade6908826e3f6dcb4c7e1c1f3024`. |
| `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/tools/sha256.rs` | SHA-256 `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e`. |
| `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a326/SP06/gateway-http/tools/select-debs.rs` | SHA-256 `54204d3c868f8bf55ab54a11c0929b6e57aa95c6229807a82b5dcc4025d85644`. |
| `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a349/SP11/real-pg-100k-review/live/run/catalog.json` | Sealed catalog input. Schema hash `eae55fc9a122759d06204b86b0147a1ef9a1066470156039362d7dd52d851b74`. |

Archived gateway sources are the fixed-base blobs, including `crates/oo-gateway/src/bin/query-pg.rs` SHA-256 `b6cd2f4e5a5af9d895a6c2ab9020ca6956afc4f61dd0b799a9cc297f54c13a22` and `tools/gateway-token-store` at the hashes in that commit's `build.rs`.

## Images

- Rust `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5` linux arm64. Not pulled.
- Postgres `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb` linux arm64. Not pulled.

## Evidence layout

`smoke-before/`, `build/`, `failed-attempts/` (`build-1`, `build-2`, `build-3`, `querypg-1`, `querypg-2`), `querypg/`, `pg/`, `live/`, `observation/`, plus `REPORT.md`, `FINDINGS.md`, `HANDOFF.md`, `SOURCE-INVENTORY.md`, `CLEANUP.md`, and `SHA256SUMS`.

File hashes for the owned tree are in `SHA256SUMS`. That file lists every other file under the crate and this evidence directory. It does not list itself.
