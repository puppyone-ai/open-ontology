# REPORT

Session `eef831aa-dba4-4af2-a78b-541bc9e0a404`. Independent review of the release and rollback card. Verdict: **WITHHOLD**. `full_sp08_accepted` is false. SP08 is not accepted. The parent goal is not accepted. No production signature. `customer_approval` stays `not_issued`.

This session did not edit `tools/release-control-api`, the identity crate, the producer, the ledger, the plan, the frontend, or a root manifest. The compiler input was `git archive` of four pinned trees. The host identity worktree was dirty and was not compiled.

## Reviewed trees

Recorded in `linux/archive/trees.txt` before `docker run`. Worktree `HEAD` at that moment was `18b0fcde61ea269cc27e2dc34a81c07ba413a30e`. The product diff of that commit against the pin was empty.

| Tree | Commit | Path | Tree SHA |
| --- | --- | --- | --- |
| product | `fbd0467617a3c1514e7ffbef52f1138e20416ac6` | `tools/release-control-api` | `7375587fb4758d41ef02727451daf3473c29cac4` |
| identity | `a6f92da8a0bf872f8a30efb116c2c7b14463505b` | `tools/control-identity` | `45f330af6e326d8f1cc1038ef47de21031ef751a` |
| producer | `9d5df65b63d3fe6c94d92d02c41d67b2b5c8aed3` | `tools/release-coordinator` | `5a3692de6f1e5609d684164a34af672b11c163b6` |
| oo-builder | `fbd0467617a3c1514e7ffbef52f1138e20416ac6` | `crates/oo-builder` | `40f74d7e9aac9c13e95d1da3d61d689184773c77` |

Frozen contract blob `dc64a53f5f54b8f4b080ecaa8561a16605d65d34` (`tools/release-control-api/CONTRACT.md`). Identity client blob `57d4eeb050886877977dfe677d8022e23993ca41` (`src/identity.rs`). Root lock snapshot `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`. `cargo-update-invoked=no`. Child lock checksums were present in that root lock.

A later commit `e2bd526e96175efb52fa1181843fbe991ae7992b` (`release-control-api: bind mutations to identity current without consume`, 2026-10-08 17:35:02 +0800) rewrites the frozen CSRF sentence. It was not in the archive and was not compiled. This review does not accept it.

## Machines

Linux arm64. Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. One builder, 6 GiB, 1 CPU, `--cap-drop ALL`, `--pull never`. One private PostgreSQL 17.10, 1 GiB, 1 CPU, no host port. `pg-wrap.sh` from `b91b67bee0f603725b2cb7cc9c284588a2699392` `deploy/trusted-demo/pg/pg-wrap.sh`, content hash `05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a`. The harness does not execute `up.sh`.

Capacity at builder start (`linux/build-1/capacity-build.txt`): `own=0 global=2 rust_images=2 host_tools=0`. Host `cargo` and `rustc` were not used. Smoke ran inside that builder before Clippy and tests.

`linux/smoke/console.log`: `rustc 1.94.0 (4a4ef493e 2026-03-02)`. `linux/raw/rustup-show.log`: `1.94.0-aarch64-unknown-linux-gnu`.

`linux/build-1/docker-run.exit` is `0`. `linux/build-1/hostconfig.txt`: `oom=false exit=0 privileged=false memory=6442450944 cpus=1000000000`. Console ends `BUILD_OK`.

Phase exits in `linux/raw/raw-exits.txt` are all `0`: `rustc-version`, `cargo-version`, `rustup-show`, `lock-checksum`, `cargo-fetch`, `rustup-clippy`, `identity-fetch`, `cargo-clippy`, `csrf-probe`, `cargo-test`, `observe`.

Clippy: `cargo clippy --locked --offline --all-targets -- -D warnings`, finished in 1m 41s, exit 0, no warning line in `linux/raw/cargo-clippy.log`.

`cargo test --locked --offline -- --test-threads=1`: library 4 passed, binary 0, contract 5 passed, live 1 passed (`private_postgres_publish_and_rollback`, 9.06s), doc-tests 0. The independent probe `csrf_rotation_and_consume_side_effect` passed in 0.63s on database `oo_a404_csrf`. `observe_product_database` passed in 0.16s on `oo_a404`.

PostgreSQL before removal: container `9d36222b1560dd7cdd860d08e45e8bdb1611923aff148643eedbfd768b09076d`, `pg/up.exit` `0`, server `17.10 (Debian 17.10-1.pgdg12+1)`, `memory=1073741824`, `cpus=1000000000`, `privileged=false`, `oom=false`. Ports `{}`. Capability drop `ALL`, add `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`. Bootstrap stdout is five `CREATE ROLE`, two `CREATE DATABASE`, and four `GRANT` tags. Password bytes were absent from that stdout, stderr, and the evidence tree.

## HTTP proof on this run

`linux/proof/private-pg.json`: `production_ready` false, `full_sp08_accepted` false, `customer_approval` `not_issued`, `test_scope` `not_production`. Thirty-one steps. The ones that carry the card:

- Active pointer starts null. Pack read is 409 `inactive`.
- No cookie 401 `unauthenticated`. Duplicate key and waiver key 400 `malformed`.
- Bad origin 403 `origin.rejected`. The frozen test's single CSRF step is 403 `csrf.rejected`. Wrong revision 409 `revision.conflict`.
- Missing operation and cross-project current are both 403 `forbidden` detail `scope`.
- Omitted G7 is 200 `refused` / `not_ready`, version null. Fabricated fence is 200 `refused` / `not_ready`, version null. Production scope on the empty pointer is 200 `refused` / `production_blocked`, version null.
- Test publish `ver-n` succeeds, class `test_scope_not_production`. Replay of that key is 202 `replay`. A different body is 409 `idempotency.conflict`. A null base while `ver-n` is active is 409 `pointer`. Production on top of `ver-n` is refused and the recorded version stays `ver-n`.
- Test publish `ver-w` succeeds. Auditor `getActiveRolePack` is 200. Reviewer pack is 403 `capability`. Query role mismatch is 403 `role`.
- Rollback of the wide publish returns `rolled_back` / `not_issued` at `ver-n`. A later narrow publish `ver-n2` refuses a wider rollback with 409 `compat.refused` detail `permission`, version `ver-n2`.
- Restart of a running row with no candidate is 200 `resume_uncertain` at `ver-n2`. A queued row completes after restart at `ver-restart`, class `test_scope_not_production`.

`linux/csrf/observe.json` after that test: reader `version` is `ver-restart`, `production_ready_certificate` false, `full_sp08_accepted` false. Identity audit on `oo_a404` includes `bootstrap_rejected` / `bootstrap.rejected` with `n` 16. `candidate_file` count 81, `pg_policy` count 0, `relrowsecurity` false.

## CSRF, measured apart from the product test

`linux/csrf/probe.json` is an HTTP client against archived identity `a6f92` on database `oo_a404_csrf`. Tokens are not in the file.

Two `GET /control/v1/identity/current` calls both returned 200. Their CSRF sha256 values differ. The stored `session.csrf_hash` equals the sha256 of the second token.

A consume of the unknown 43-character token `A` repeated, with the first token and the real origin, is 403 `csrf.rejected`. Audit delta 0. Stored hash unchanged.

The same consume with the second token and origin `https://127.0.0.1:9` is 403 `origin.rejected`. Audit delta 0. Stored hash unchanged.

The same consume with the second token and the real origin is 403 `bootstrap.rejected`. Audit delta 1. Stored hash unchanged and still equal to the sha256 of the presented token. Bootstrap states are exactly `["consumed"]` (the login row). The unknown token inserted no bootstrap row.

A third `GET /control/v1/identity/current` returns a new token and the stored hash follows it. Reusing the second token is then 403 `csrf.rejected` with audit delta 0.

That is the frozen contract's missing comparison, and it is the side effect the product run left behind as 16 `bootstrap_rejected` rows.

## What a green test does not close

The live test passed. The frozen sentence in blob `dc64a53f` still says a mutation's `X-OO-CSRF` equals the current envelope `csrf_token`. The code under test posts `POST /control/v1/auth/bootstrap/consume` with a constant unknown token and treats a `bootstrap.*` 403 or 409 as success. Fix items are in `FINDINGS.md`. No customer gate was presented. This review does not sign production.
