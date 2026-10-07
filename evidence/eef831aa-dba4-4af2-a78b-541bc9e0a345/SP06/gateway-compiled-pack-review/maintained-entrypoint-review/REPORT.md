# Maintained entrypoint review

Verdict: APPROVE_LIMITED for the test repair at `99e93d0f88eb95d8b3834c534dc8ae90d62753ec`.

The sealed RolePack loader remains the prior limited approval of `6001e18ee15d7f6a12d7e5f9c9cd89c8cd8a7b5c`. This packet does not accept E45, the parent gateway goal, public TLS, OAuth, an IdP, a readiness database probe, durable audit, SQL execution, or runsc.

The commit message is `gateway: seal the maintained live test with role-pack-bundle`. Parent `94f7c1668d89d628b0e3103fc9f26e3f2a1945f6`. `git diff-tree` names only `crates/oo-gateway/tests/http_gateway_live.rs`. Content SHA-256 `59e0a940f6fed9f63e7ebc0da37ddc189f287bab17e50ca794b5316eb41c9059`. `pack_catalog.rs`, `canonical.rs`, `role_pack.rs`, `http_gateway_pg_live.rs`, `fixture.rs`, `CONTRACT.md`, `Cargo.toml`, and `Cargo.lock` are the same blobs as `6001e18e`. `compile_pack` in the repaired test is byte-identical to `compile_pack` in `http_gateway_pg_live.rs` (1221 bytes). The diff adds no HTTP assertion and no `5_200` sleep. Both existing sleeps remain, at lines 120 and 135. `fixture_artifact(` does not appear. The new code calls the fixture `compile` command with `role-pack-bundle`, checks both digest files, and deletes `picker.json.digest` and `auditor.json.digest` before `write_config` returns. The pack directory is asserted to contain only `auditor.json` and `picker.json`.

That is not a forged operator pack. `fixture_artifact` still returns `legacy_unsupported` because `pack_catalog.rs` was not edited.

`http_gateway_pg_live` was not run again. There was no concrete concern in the loader, the compiler, the lock, or the copied `compile_pack`. The prior a345 `linux/smoke-3` proof of cache, revoke, outage, bounds, and process refusal stays the evidence for those clauses. `linux/smoke` and `linux/smoke-2` from that review are unchanged.

## What ran

Fixed archive `99e93d0f`, Linux arm64, Rust 1.94.0, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. No pull. No overlay. Container `1913115e6bbbd24c70b0da2ad579b58a59ed4ef1b5ec461ae6ac622fe8efd103`, name `oo-a345-entry-proof`, removed by `launch.sh`.

`--cap-drop ALL`, `no-new-privileges`, `Privileged=false`, memory `6442450944`, `NanoCpus=1000000000`, `OOMKilled=false`. Network `oo-a345-entry-net`. `HOME=/tmp`. Private cargo and target volumes. `/tmp` tmpfs `rw,nosuid,nodev,noexec,size=1g`. `/src` read-only. Capacity at start: `own=0 global=0 host_cargo=0 own_pg=1`.

PostgreSQL `oo-a345-entry-pg` `34b02145d394eb14d707692b0cf5eed7e3643f3ab338c42d256bb0b093846d55`. Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, arm64. Memory `1073741824`, `NanoCpus=1000000000`, `Privileged=false`, `CapDrop ["ALL"]` plus `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`. Published ports `{}`. Database `oo_a345`. `roles.txt` ends `roles_ready`.

| Check | Exit | Result |
|---|---|---|
| archive SHA-256 of the live test, `pack_catalog.rs`, `Cargo.lock`, `fixture.rs`, and `CONTRACT.md` | 0 | matched the commit |
| `cargo test --test http_gateway_live` without `--ignored`, with `OO_A326_LIVE=1` | 0 | 0 passed, 1 ignored, marker absent |
| `cargo clippy -p oo-gateway --bin oo-gateway --lib --test http_gateway_live -- -D warnings` | 0 | stdout empty, finished in 1m 24s |
| fixture `rustc` and `role-pack-bundle` | 0 | archive sources |
| `OO_A326_LIVE=1 --test http_gateway_live -- --ignored` | 0 | 1 passed in 11.36s |
| marker | 0 | `ran=1` then `completed=1` |
| `Cargo.lock` | 0 | unchanged |
| secret scan of the generated logs | 0 | |
| harness | 0 | `inside-ok.txt` `2026-10-07T18:42:48Z` |

`docker-run.exit=0`. A default `cargo test` still does not run this entrypoint. The passing result is the `--ignored` command.

Workspace HEAD during the proof was `35e77c96919f5ef22f9f2a185fd883a53d67c6d9`. The archive came from `git archive` of `99e93d0f`, including `testing/real-db/gateway-compiled-pack`. The dirty workspace was not mounted.

## Out of this approval

Public TLS, OAuth, an IdP, a readiness database probe, durable audit, SQL execution, runsc, and the parent gateway goal. The 1-second warm-hit check is not in this file. It remains in the previously approved `http_gateway_pg_live` proof.
