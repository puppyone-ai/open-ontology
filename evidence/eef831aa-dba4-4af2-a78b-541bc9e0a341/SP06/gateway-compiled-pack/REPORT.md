# Report

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a341`. Original GW-D02 / E45. This card replaces the operator pack catalog with a loader for a sealed `oo-builder` role pack. It is not an acceptance of E45, a parent-goal acceptance, or a release.

Product commit `6001e18ee15d7f6a12d7e5f9c9cd89c8cd8a7b5c`. The passing proof is `linux/smoke-3/`. Container `c9ce1a4ea3a707c2c1a4a2a934bbca49a57ba9f1eb2d3ec1f6c5791e351c4e96`. `inside_ok=2026-10-07T16:44:56Z`. `docker-run.exit=0`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `rustc 1.94.0 (4a4ef493e 2026-03-02)` `aarch64-unknown-linux-gnu`. `cargo 1.94.0 (85eff7c80 2026-01-15)`. Limits: 6442450944 bytes, 1 CPU, `CapDrop=["ALL"]`, not privileged, network `oo-a341-eef831aa-net`. OOMKilled false. Archive commit `cd8f754c1b4db22fc962370c4b29bb846b0fccc3`. `Cargo.lock` compared equal after every cargo invocation (`lock-unchanged.exit=0`).

PostgreSQL was the existing `oo-a341-pg` (`94815b96965c439ec2f836c4d265fa932645b4cb81d2a9818e489ed522d28bf0`), image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, 1 GiB, 1 CPU. The smoke-3 builder reused it. No second a341 database was created.

## What the loader does

`PackCatalog::load_dir` reads artifact JSON produced by `RolePack::seal` at `cd8f754`. It does not call `oo-builder` and `oo-gateway` does not depend on that crate. `canonical_json` in `pack_catalog.rs` is the compiler writer from `crates/oo-builder/src/canonical.rs` at that commit. The digest is SHA-256 of the canonical array of `{bytes, path, sha256}` for every file, including `manifest.json`, sorted by path. The same algorithm is `payload_sha256` in `oo-core`.

A directory is refused before `listen.addr` when the compiler field is absent (`legacy_unsupported`), the compiler string is wrong (`compiler`), a file or manifest digest does not match (`source_digest`), a path repeats (`path_duplicate`), a ceiling or declared total disagrees (`budget`), the declared `pack_digest` disagrees, including `ab` repeated 32 times (`pack_digest`), or the README role, an extra `oo_r_` token, or a table cell `unknown` disagrees (`role_mismatch`). Project, ontology, catalog, policy, and knowledge binding failures use `binding`. `fixture_artifact` always returns `legacy_unsupported`. The HTTP tuple still comes from the accepted token store. Metadata returns the recomputed digest and `{path, bytes, sha256}` without file text.

## Proof

The immutable archive was tested before the overlay on every attempt. Baseline on `cd8f754`: `pack_catalog` 2 passed, `http_gateway_loopback` 8 passed. After the overlay, `linux/smoke-3` recorded:

| step | exit | result |
|---|---|---|
| `cargo fetch --locked` | 0 | registry on the private cargo volume |
| baseline `pack_catalog` | 0 | 2 passed |
| baseline `http_gateway_loopback` | 0 | 8 passed |
| `rustc` fixture tool | 0 | std-only driver |
| `cargo build -p oo-builder --bin role-pack-bundle` | 0 | compiler from the archive |
| overlay `pack_catalog` | 0 | 6 passed |
| overlay `http_gateway_loopback` | 0 | 8 passed |
| `http_gateway_pg_live` compile | 0 | 1 ignored |
| `cargo clippy -p oo-gateway --all-targets -- -D warnings` | 0 | `Finished` in 1m 32s |
| `http_gateway_pg_live --ignored` | 0 | 1 passed in 11.44s |

The live test compiled two packs with `role-pack-bundle` from the public fixtures, refused the hidden visibility with `classification.unknown_pii`, and checked that `unclassified_pii` is absent from both artifacts. Picker metadata returned `order_id` and `picker_note`. Auditor metadata returned `auditor_phone` and `order_id`. HTTP file SHA-256 values matched both the artifact declarations and SHA-256 of the file text. Replacing `pack_digest` with `ab` repeated 32 times, and removing `compiler`, exited 1 before `listen.addr` with stderr `oo-gateway: packs`. `load_dir` also refused a zeroed file SHA-256, a duplicate path, and `total_bytes` of 1.

The same process kept the 5 second positive cache. After revoke, the request at 1 second from the first 200 stayed 200, and the request at 5.2 seconds returned 401. A restart stayed 401. With the endpoint rewritten to `127.0.0.1:1`, the warm request at 1 second stayed 200 and the request at 5.2 seconds returned 401. `/ready` kept `database=not_probed`. A restart during the outage returned 401, and restoring the endpoint returned 200. Forged `X-OO-Role`, `X-OO-Project`, and `X-OO-Scope` returned 403. A session token and a missing bearer returned 401. A query string returned 400. `POST /v1/sql` returned 501 with and without a bearer. The live marker is `ran=1` then `completed=1`. Gateway stderr and `/proc/<pid>/cmdline` did not contain the runtime password or an `ooct_` / `oost_` prefix.

`http_gateway_loopback.rs` was not edited. `OO_A326_LIVE` was not set. `fixture_artifact` no longer builds an operator pack, so that ignored a326 test fails if it is enabled.

## Earlier attempts kept

| directory | container | exit | stop |
|---|---|---|---|
| `linux/smoke/` | `29d0ff1fe28e3b6c734dab548e2ab605a0bf9d6b0c3169a3e872e071eeadd7e8` | 1 | fixture `rustc` `E0716` after a green archive smoke |
| `linux/smoke-2/` | `58f5f5e05e18572179ae8f5dd1491c408f789547cf3ff3a1c01c5efeaa84b534` | 101 | `pack_catalog.rs:838` called a shadowed `String` |

Both runs used the same Postgres. Their console logs and exit files were not overwritten.

## Not claimed

External mTLS, OAuth, an IdP, a PostgreSQL readiness probe, durable audit, runsc, and SQL execution are unchanged and are not claimed. The parent plan checkbox for E45 stays open until an independent review accepts this commit against the frozen compiler-artifact contract.
