# Gateway compiled-pack review

Verdict: APPROVE_LIMITED.

Reviewed commit: `6001e18ee15d7f6a12d7e5f9c9cd89c8cd8a7b5c` ("gateway: verify sealed role-pack compiler digests").
Review: `eef831aa-dba4-4af2-a78b-541bc9e0a345`.
Workspace HEAD during the proof: `73e63d261615111dcdefa98c25b17b1afb4d88f3`.
`git diff 6001e18e` for `pack_catalog.rs`, the three gateway HTTP tests, `canonical.rs`, `testing/real-db/gateway-compiled-pack`, `Cargo.toml`, and `Cargo.lock` was empty. The proof used `git archive` of `6001e18e`, mounted read-only.

This approves the sealed RolePack loader at that commit for the checks below. It does not accept E45, the parent gateway goal, public TLS, OAuth, an IdP, a readiness database probe, durable audit, SQL execution, or runsc.

The maintained entrypoint `crates/oo-gateway/tests/http_gateway_live.rs` fails when `OO_A326_LIVE=1` and `--ignored` is set. A default `cargo test` leaves that test ignored and exits 0. That default result is not a pass. The exact test-only repair is in HANDOFF.md. Product files were not edited.

## What was approved

Immutable archive, Linux arm64, Rust 1.94.0, before `review_pack.rs` was copied:

| Check | Exit | Result |
|---|---|---|
| `pack_catalog` unit tests | 0 | 6 passed |
| `http_gateway_loopback` | 0 | 8 passed in 0.47s |
| `http_gateway_pg_live` without `--ignored` | 0 | 0 passed, 1 ignored |
| `http_gateway_live` without `--ignored`, `OO_A326_LIVE=1` | 0 | 0 passed, 1 ignored |
| `cargo clippy -p oo-gateway --all-targets -- -D warnings` on `/src` | 0 | finished |
| `Cargo.lock` versus the archive copy | 0 | unchanged |

`fixture_artifact` in the archive still ignores its arguments and returns `legacy_unsupported`. `old-entrypoint.txt` records the two call sites at lines 17, 235, and 236.

Fresh proof on the same archive, private PostgreSQL `oo_a345`, after the review file was copied into `/target/work` only:

| Check | Exit | Result |
|---|---|---|
| `http_gateway_pg_live --ignored` | 0 | 1 passed in 11.43s, `live-marker.txt` `completed=1` |
| `http_gateway_live --ignored` with `OO_A326_LIVE=1` | 101 | panic at `http_gateway_live.rs:235:98`, `PackError { reason: "legacy_unsupported" }`, 0.19s, marker stayed `ran=1` |
| `review_pack` | 0 | 1 passed in 0.19s |
| `cargo clippy -p oo-gateway --all-targets -- -D warnings` on the work tree | 0 | finished in 8.79s |
| secret scan of generated logs | 0 | |
| `Cargo.lock` | 0 | unchanged |
| harness | 0 | `inside-ok.txt` `2026-10-07T17:29:00Z` |

The passing live test is the approved campaign, reused rather than extended. It compiled picker and auditor packs with `role-pack-bundle`, refused hidden visibility with `classification.unknown_pii`, checked both digests differ, and served token HTTP metadata for `order_id` plus `picker_note` versus `auditor_phone` plus `order_id`. Metadata carried `project_id=depot`, `policy_version=pv1`, `knowledge_version=kv1`, `authentication=loopback-http`, and `external_mtls` / `oauth` / `idp` false. `/ready` stayed `database=not_probed`. A warm hit at 1s stayed 200. Revoke and outage both became 401 at 5.2s and stayed 401 across a restart. Forged role, project, and scope headers were 403 `FORBIDDEN`. A session token and a missing bearer were 401. A query string was 400 `QUERY_NOT_ALLOWED`. `POST /v1/sql` was 501 `SQL_UNSUPPORTED` with and without a bearer. Load-dir refusals covered `pack_digest` (`ab` repeated 32 times), missing `compiler` (`legacy_unsupported`), a zeroed file SHA-256 (`source_digest`), a duplicate path, and `total_bytes=1` (`budget`). The process spawned `oo-gateway` and refused, before `listen.addr`, for the declared-digest tamper and for a missing compiler key. Stderr contained `oo-gateway: packs`.

The review oracle, which is not a product test, recomputed the compiler preimage with its own canonical writer. Both role digests matched the compiler digest files. The manifest kept project `depot`, policy `pol_depot@pv1`, knowledge `know_depot@kv1`, compiler `oo-builder/role-pack/v1`, and the two role names. Neither artifact contained `unclassified_pii`. It then spawned `oo-gateway` three more times. A compiler string `other-compiler` refused with reason `compiler`. A zeroed file SHA-256 refused with reason `source_digest`. A same-length README project swap from `` `depot` `` to `` `other` ``, with that file's SHA-256, `manifest_sha256`, and `pack_digest` resigned while `"project":"depot"` stayed in the manifest, refused with reason `binding`. Each child exited 1, wrote no `listen.addr`, and its stderr contained `oo-gateway: packs`. `review-summary.txt` records those lines.

`write_value`, `write_object`, and `push_string` in the archive gateway match the compiler writer. `push_string` differs only by the compiler's `pub` keyword. See `source/canonical-compare.txt`.

Author manifest recheck: `source/a341-manifest-check.txt` records 188 entries, 188 ok, 0 failed, from `shasum -a 256 -c` of the a341 `SHA256SUMS`. That check is not the digest oracle for this proof.

## Platform

Smoke-3 container `210206936d202c11ed60caa12b4ac832435610897031216d7a6b567c2e040016`, removed by the launcher as `oo-a345-proof-3`.

- Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `os=linux`, `arch=arm64`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. No pull.
- `--cap-drop ALL`, `no-new-privileges`, `Privileged=false`, memory `6442450944`, `NanoCpus=1000000000`, `OOMKilled=false`.
- Network `oo-a345-eef831aa-net`. `HOME=/tmp`. Private cargo volume `oo-a345-eef831aa-cargo` and target volume `oo-a345-eef831aa-target`. `/tmp` tmpfs `rw,nosuid,nodev,noexec,size=1g`.
- `/src` and `/fixture` read-only. a326 hash tools bind-mounted read-only. Review test bind-mounted read-only at `/review/review_pack.rs`.
- Capacity at start: `own=0 global=1 host_cargo=0 own_pg=1`. The one global builder was foreign container `oo-a339-proof7` (`oo.owner=a339`). It was not removed.

PostgreSQL `oo-a345-pg` `92d27e03fab0351e33fd6c6853b8d8573b4211158ea7b930710b82f1685cdd10`. Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, arm64. Memory `1073741824`, `NanoCpus=1000000000`, `Privileged=false`, `CapDrop ["ALL"]` plus `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`. Published ports `{}`. Database `oo_a345`. `pg_isready` was accepting connections before the proof.

`invocation.txt` is a summary line. Secret values stayed in files under `/tmp/oo-a345-eef831aa-dba4-4af2-a78b-541bc9e0a345/`. The builder fixture had no key and no `server.crt`.

## Failed attempts kept

`linux/smoke` container `f99d6d26530ed7f51a89ee6c73b7f423f618d780f6a9180102690b6e4e33bebc`, `docker-run.exit=101`. Archive smoke, Postgres live test, and the old-entrypoint failure matched the final run. `review_pack` panicked because the README rebind updated the file SHA-256 and `pack_digest` but left `manifest.json` listing the old README SHA-256. `load_dir` correctly returned `source_digest` before the project-line check. Compiler-string and zeroed-file process refusals in that run had already passed. `linux/smoke/RETRY.txt` records the oracle fix. The failing `.rs` bytes were overwritten before they were snapshotted. Product files were not edited.

`linux/smoke-2` container `c414aaa8f198479da2a1d5cce2a95cba36ee7a04455337a8a5c1b99e22e0fd2a`, `docker-run.exit=101`. The same product checks passed, including `review_pack` with `binding_reason=binding`. Clippy `-D warnings` then failed on the review test only: `review_pack.rs:45` `clippy::useless_format`, a `format!` with no placeholders. Archive Clippy on `/src` was already 0. The constant was changed to a string `.to_string()`. No `#[allow]`. Product files were not edited.

`linux/smoke-3` is the run whose harness exited 0. Its `old-live.exit` is still 101. A green harness exit means the expected regression was observed and the later checks passed. It does not mean every test passed.

## Out of this approval

- `http_gateway_live` under `OO_A326_LIVE=1 --ignored` (failed, repair required).
- Public TLS, OAuth, IdP, readiness database probe, durable audit, SQL execution, runsc.
- E45 and the parent gateway goal. a337's limited approval was the operator catalog. This contract requires a compiler digest. `ab` repeated 32 times is refused here.
- Workspace dirt outside the archive (`crates/oo-builder/src/description_candidate.rs`, `testing/spikes/p06-p13/`) was not reviewed and not changed.
