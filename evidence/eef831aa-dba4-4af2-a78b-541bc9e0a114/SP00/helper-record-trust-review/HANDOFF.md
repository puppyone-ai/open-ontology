# Handoff — a114 helper record trust review

**Decision: APPROVE LIMITED for F1–F4. Records stay `audited=false`. Do not set `audited=true`, `admission`, `fresh_current_scan`, or `release_signoff` from this review.**

The fixed checker accepts the five unaudited records and rejects the paired adoption and age forgeries, the paired Apache choice, an aligned unverified audit status, blank and numeric status, a removed vulnerabilities object, and a paired RustSec identity. Measured container depth 64 is accepted. Measured depths 65 and 20000 return exit 22 with no signal. All-target clippy remains red on the pre-existing `timeutil.rs` assertion. That command is not part of the smoke total and is not a green result.

Unknown keys, hard links, manifest binding, pre-stat time-of-check/time-of-use, hash-subprocess timeout, and the hash child's environment stay unsigned. This handoff does not certify hostile-input handling, a full license reading, a legal conclusion, a fresh scan, or a release.

## Bound input

| | |
| --- | --- |
| Session | `eef831aa-dba4-4af2-a78b-541bc9e0a114` |
| Reviewed commit | `6d2f7cdb935c7debad0dbb5292074f325d1948cb` |
| Tree | `dad005c861fca7fd906f49551169777a37b1c5d0` |
| Parent | `d67795e397657087495a32317938ecdd7a6cb028` |
| Subject | `harness: pin helper record trust and json depth` |
| Helper-lock commit | `f1a6005971e3dc3075094c87ed670fc9d25e9ae0` |
| Adoption instant | `2026-10-02T11:10:37Z` |
| Lock sha256 | `14a0a0c66d604bae81f59b770e66a2d7cf00b38e427c3afbc8344bb4864189c1` |
| Lock blob | `2152785a3c9dc6713a62eb18052d7224bb5357eb` |
| Run | `9910ff22-66f3-49c6-82e4-bea42642b58f` |
| Workspace tip at launch | `e5a066622e0409762fe44f11c4d303d2468a3290` |

The smoke compiled `git archive` of `6d2f7cdb` for the checker, the helper manifest and lock, provenance, `ci/deny.toml`, and the a101 raw inputs. The workspace tip and the connector tree were not compiled. Checker, provenance, helper lock, and `ci/deny.toml` are unchanged from `6d2f7cdb` to the launch tip.

This file is inside the evidence commit, so it cannot name that commit's own SHA.

## What this run showed

Rust 1.94.0, linux/arm64, image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Container `6c5b279f1b4cc71a233f26e6c0d60198b28dfa6a3053dbf698e5cdb12d420e36`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. `airgap_claim=no`: clippy was downloaded. Test, scoped clippy, and the probe then ran `--locked --offline`.

`cargo test --locked --offline`: 7 unit, 25 closure, 11 trust, 0 failed. Lib/bin clippy exit 0. `cargo clippy --test trust --test closure -D warnings` exit 0. `cargo clippy --all-targets -D warnings` exit 101 at `timeutil.rs:112` `clippy::assertions_on_constants`. `smoke.exit` is 0 and does not include that 101.

Independent probe `matched=22 case_mismatches=0 other=0 missing=0 cases=22`. Checker binary `2cb04ba6701778408a5c09839af053c89d50614510546dff7a9e4ae1ec6adad7`.

Rows to read first, all `signal=none`:

- canonical five: exit 0, `audited=false`
- paired adoption and recomputed ages: exit 8 `invalid-time` `adoption anchor`
- real adoption, forged agreeing ages: exit 9 `age-floor` `bitflags`
- paired Apache choice, inventory plus records plus nix native entries: exit 4 `wrong-dual-choice` `bitflags`
- `status=unverified` with a rewritten digest: exit 19 `fake-claim` `audit status`
- blank status, numeric status, removed vulnerabilities object: exit 21 `malformed-record`
- paired RustSec commit across audit, advisory rev, inventory, and records, digests realigned: exit 19 `fake-claim` `audit database identity`
- object, array, and mixed at measured depth 64: exit 0
- the same three shapes at measured depth 65 and 20000: exit 22 `bound`
- scalar root measured depth 0: exit 21 `missing package`
- bad escape exit 21, invalid UTF-8 exit 21, duplicate key exit 14

Phase exits: identity 0, pin 0, cargo-test-offline 0, clippy-component 0, clippy-offline 0, clippy-tests-offline 0, probe-build 0, probe 0, smoke 0, docker-exec 0, driver 0, clippy-all-targets 101.

## Retained RED

a109, unfixed sources equal to `abd074af`, binary `c2a7b9bae81198010c0cfa6b682a71087a2df3541c14c16fd7df7f17d4ca8694`: paired adoption exit 0, paired Apache exit 0, depth-64 observe exit 0, depth-20000 observe exit 134 abort, unverified audit status exit 0. The 46-case matrix was not rerun. a113 attempts 1–3 stay red in their own logs.

## Capacity and cleanup

Slot B. Before launch: host cargo/rustc/rustup/rustdoc 0, running rust-named containers 0, total 0. The only running container was `eef831aa-dba4-4af2-a78b-541bc9e0a112-pg`, which is outside the rust count. One a114 Rust container. No PostgreSQL client was started by this review.

Removed only this run: container `eef831aa-dba4-4af2-a78b-541bc9e0a114-rust`, network `oo-a114-9910ff22-66f3-49c6-82e4-bea42642b58f-net`, the two run volumes, and `/tmp/oo-a114-9910ff22-66f3-49c6-82e4-bea42642b58f`. `docker-ps-owned-after.txt` is empty. No prune. After cleanup, a112 `pg` was still Up, a112 `rust-r1` and `rust-base` were still exited, and the a064 containers were still exited.

## Coordinator

F1–F4 are accepted inside the limits in `FINDINGS.md`. Leave `audited=false`. Checklists stay with the coordinator. This handoff does not sign SP00, AT-SEC-05, or a release.
