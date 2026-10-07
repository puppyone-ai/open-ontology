# Maintained live-test repair

The `OO_A326_LIVE` test now loads compiler-sealed RolePack spans. E45 stays open. This packet does not accept the parent gateway goal, public TLS, OAuth, an IdP, a readiness database probe, durable audit, SQL execution, or runsc.

Product commit: `99e93d0f88eb95d8b3834c534dc8ae90d62753ec` (`gateway: seal the maintained live test with role-pack-bundle`). Parent: `94f7c1668d89d628b0e3103fc9f26e3f2a1945f6`. The commit contains only `crates/oo-gateway/tests/http_gateway_live.rs`. Its SHA-256 is `59e0a940f6fed9f63e7ebc0da37ddc189f287bab17e50ca794b5316eb41c9059`, the same bytes the proof overlaid.

Proof archive: `93f5b8aea78d0d745334da0bc5352afdb86412e2`. `pack_catalog.rs` blob `9848dcafc4c36b6512240f20fec1b09b1146c02a` matches `6001e18ee15d7f6a12d7e5f9c9cd89c8cd8a7b5c`. `git diff` from the archive commit to the product parent is empty for `oo-gateway`, `role_pack.rs`, `canonical.rs`, `role-pack-bundle.rs`, `Cargo.toml`, and `Cargo.lock`. `testing/real-db/gateway-compiled-pack` and the a341 evidence packet were not edited.

Contract `CONTRACT.md` SHA-256 `b3b3a6daaaed24c5e74435ede846c409fa0ea876a6ffadca5f340f3e6be6340f` was written before the test edit.

## What ran

Linux arm64 Rust 1.94.0, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. Cargo `1.94.0 (85eff7c80 2026-01-15)`. No pull. Container `41a498d09b23449dba66e144cf4751c22dab96c5088a0807071e7c9fe81ec313`, name `oo-a341-live-repair`, removed by `launch.sh`.

`--cap-drop ALL`, `no-new-privileges`, `Privileged=false`, memory `6442450944`, `NanoCpus=1000000000`, `OOMKilled=false`. Network `oo-a341-eef831aa-net`. `HOME=/tmp`. Private cargo and target volumes. `/tmp` tmpfs `rw,nosuid,nodev,noexec,size=1g`. Capacity at start: `own=0 global=0 host_cargo=0`.

PostgreSQL `oo-a341-pg` `7b3f3e4548e348fcf941f50863ed8bd033f5189f9578a89f3709533051b3e791`. Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, arm64. Recipe memory `1g`, swap `1g`, 1 CPU. `Privileged=false`, published ports `{}`, database `oo_a341`. `up.exit=0`. `roles.txt` ends `roles_ready`.

Pre-edit smoke on the immutable archive, before the overlay:

| Check | Exit | Result |
|---|---|---|
| `pack_catalog` unit tests | 0 | 6 passed |
| `http_gateway_loopback` | 0 | 8 passed in 0.47s |
| `http_gateway_live` without `--ignored` | 0 | 0 passed, 1 ignored |

`archive-entrypoint.txt` records `fixture_artifact` at archive lines 17, 235, and 236. That ignored command was not executed again. a345 already recorded its panic.

After overlay of only `http_gateway_live.rs`:

| Check | Exit | Result |
|---|---|---|
| fixture `rustc` | 0 | `testing/real-db/gateway-compiled-pack/src/fixture.rs` |
| `role-pack-bundle` | 0 | archive `oo-builder` |
| focused Clippy `-D warnings` | 0 | stdout empty; stderr ends `Finished dev profile ... in 1m 23s` |
| `OO_A326_LIVE=1 --test http_gateway_live -- --ignored` | 0 | 1 passed in 11.27s; marker `ran=1` then `completed=1` |
| `OO_A341_LIVE=1 --test http_gateway_pg_live -- --ignored` | 0 | 1 passed in 11.43s; marker `completed=1` |
| `Cargo.lock` | 0 | unchanged |
| bearer-prefix scan of the four live logs | 0 | |
| harness | 0 | `inside-ok.txt` `2026-10-07T18:05:07Z` |

`docker-run.exit=0`.

Focused Clippy was `cargo clippy -p oo-gateway --bin oo-gateway --lib --test http_gateway_live --test http_gateway_loopback --test http_gateway_pg_live -- -D warnings`. It did not include `bounded_in`.

The repaired test calls the existing `compile_pack` command for `visibility-picker.json` and `visibility-auditor.json`, checks both digest files are 64 lowercase hex and differ, checks each artifact `pack_digest` equals its digest file, then deletes the digest files so `load_dir` sees only `auditor.json` and `picker.json`. The HTTP assertions and both existing `5_200` ms sleeps are unchanged. There is no 1-second warm-hit check in this file. The 11.27s duration is those two sleeps plus the surrounding requests.

The compiled-pack entrypoint was run again on the same private database. It still compiles both roles, refuses hidden visibility, and checks the 5-second cache. That campaign was not extended.

## Cleanup

Owned cleanup finished at `2026-10-07T18:06:40Z`. See `CLEANUP.txt`. Prune was not run. No foreign container was removed.
