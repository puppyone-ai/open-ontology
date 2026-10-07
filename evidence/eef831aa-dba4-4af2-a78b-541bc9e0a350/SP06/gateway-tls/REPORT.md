# REPORT — SP06-W03 TLS external metadata

Actor `a350`, session `eef831aa-dba4-4af2-a78b-541bc9e0a350`. This file records the proof-6 container results. It does not accept E48, SP06, public identity, OAuth, or a parent goal. The author is not an independent reviewer.

## Proved run

`linux/proof-6` container `ff03ce5fbe930e1bbe71a73b6f130b46b1b6880ca9d40ab98aab2e51faefa638`.

- `docker-run.exit` 0, container `ExitCode` 0, `OOMKilled` false
- `Privileged` false, `CapDrop` ALL, memory 6442450944, `NanoCpus` 1000000000
- network `oo-a350-eef831aa-net`, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, arch arm64
- rustc 1.94.0 (`4a4ef493e3a1488c6e321570238084b38948f6db`), host `aarch64-unknown-linux-gnu`
- phases end at `PHASE inside_ok` (`inside-ok.txt` `2026-10-07T21:05:04Z`)
- PostgreSQL for this attempt: `pg/up.exit` 0. The container and `oo-a350-eef831aa-pgdata` were removed by the launch cleanup. No `oo-a350` container, volume, or network remained.

## Commands and exits

Clippy, not `--all-targets` and not the rest of the workspace:

`cargo clippy --locked --keep-going -p oo-gateway --lib --bin oo-gateway --test http_gateway_loopback --test http_gateway_tls --test http_gateway_pg_live --test http_gateway_live -- -D warnings`

`clippy.exit` 0. The stderr is the compile finish line only.

| step | exit | container log |
| --- | --- | --- |
| rustc | 0 | `rustc.exit` |
| clippy | 0 | `clippy.exit` |
| `http_gateway_loopback` | 0 | 8 passed, 0 failed, 0.46s |
| `http_gateway_tls` (default) | 0 | 5 passed, 1 ignored, 1.54s |
| pack fixture rustc | 0 | `fixture-rustc.exit` |
| `role-pack-bundle` | 0 | `bundle.exit` |
| `http_gateway_live` ignored, `OO_A326_LIVE` | 0 | 1 passed, 11.32s |
| `http_gateway_pg_live` ignored, `OO_A341_LIVE` | 0 | 1 passed, 11.39s |
| `http_gateway_tls` ignored, `OO_A350_LIVE` | 0 | `verified_peer_metadata_uses_the_compiled_pack_and_the_leaf_uri` passed, 6.21s; 5 other tests filtered out |
| lock compare | 0 | `lock-unchanged.exit` |

Markers `a326`, `a341`, and `a350` each contain `ran=1` and `completed=1`.

Default TLS tests that passed: invalid material before `listen.addr`, binary exit before listen on a bad key, historical loopback config, loopback refusal of a proxy certificate header and `/internal`, and the TLS listener handshake/bounds test. The live metadata test is the ignored one and passed only in the later `OO_A350_LIVE=1` invocation.

`Cargo.lock` sha256 `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474` matches the workspace file. No manifest or lock edit.

## Source measured by proof-6

Repository HEAD at launch: `96fc054598a916737452e74bb9ddaa46180057bb`.

| file | git blob |
| --- | --- |
| `crates/oo-gateway/src/http_gateway.rs` | `969e655d7458d8b679933ffb6f361f08741cabea` |
| `crates/oo-gateway/tests/http_gateway_tls.rs` | `39958a114b0e4f84f258a90b8c06ee9cb4507aeb` |
| `crates/oo-gateway/src/bin/oo-gateway.rs` | `3fa14007bd293612771a8aecf9794a405b5a8107` (unchanged; not part of the diff) |

Work tree and archive blobs for the two edited sources were equal in `work-http-blob.txt`, `archive-http-blob.txt`, `work-tls-test-blob.txt`, and `archive-tls-test-blob.txt`.

## Earlier smoke

`linux/smoke/*.exit` are all 0 from the pre-edit run. That smoke was not rerun. Its `fixed-http-blob` is the pre-edit gateway file, not the proof-6 blob.

## Cleanup

Removed only a350 resources: container `oo-a350-proof-6`, volumes `oo-a350-eef831aa-cargo` and `oo-a350-eef831aa-target`, network `oo-a350-eef831aa-net`, and the PostgreSQL container and data volume. `/tmp/oo-a350-eef831aa-dba4-4af2-a78b-541bc9e0a350/secrets` is absent. No prune. No private key or certificate file is in this evidence tree. Token-prefix words in `CONTRACT.md` are specification text, not issued tokens.
