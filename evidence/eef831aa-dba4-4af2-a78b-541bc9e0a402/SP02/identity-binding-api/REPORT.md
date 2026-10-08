# SP02-W05/B01 identity binding loop

Session `eef831aa-dba4-4af2-a78b-541bc9e0a402`. Linux arm64, Rust 1.94.0 (`4a4ef493e`, 2026-03-02), cargo 1.94.0. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL 17.10 image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. One builder, 6 GiB, 1 CPU. One private database `oo_identity_a402` on `oo-a402-pg`, 1 GiB, 1 CPU. Dependencies stayed locked. No root manifest change. The child lock `tools/identity-binding-api/Cargo.lock` is a checksum subset of the root lock (226 packages compared, 0 mismatches).

`tools/control-identity` keeps contract `oo.control-identity/v1` and the five operations: login, callback, `getIdentityCurrent`, logout, bootstrap consume. Two routes are added: `POST /control/v1/identity/bindings/preview` and `POST /control/v1/identity/bindings/approve`. `tools/identity-binding-api` is a separate HTTP crate, contract `oo.identity-binding/v1`, routes `POST /binding/v1/preview` and `POST /binding/v1/approve`. It has no path dependency on control-identity.

Preview reads the current verified admin session and returns `planned` or `unchanged`. It does not write Git, PostgreSQL, or an audit row, and it does not rotate CSRF. Approve on a planned change writes one Git commit with `update-ref` compare-and-swap, then control-identity checks that commit and, in the same PostgreSQL transaction, updates `binding_head`, the one management binding, the `binding_applied` audit row, and the idempotency receipt. `GET /control/v1/identity/current` builds `projects` from those rows. A session stamped with the previous revision receives `409 revision.conflict` and its idle time is left in place. Logout with the CSRF from before the apply still returns `200`. The binding is still there after the identity process is killed and started again.

Public changes require the current session cookie, `Origin` equal to the service origin, header `X-OO-CSRF`, the admin binding on that project, and the expected revision, config commit, and Git head. `authorized` in the JSON body is `400 malformed`. IdP role and group claims are not read. A management role does not issue data grants, an owner certificate, a harness session, or a Gateway token. `customer_integration` stays `not_claimed`.

## Final exits

| Check | Exit | Result |
| --- | --- | --- |
| Pre-edit control-identity lib | 0 | 11 passed (`smoke/`) |
| Pre-edit control-identity cli | 0 | 1 passed |
| control-identity lib, after the change | 0 | 14 passed |
| control-identity cli | 0 | 1 passed |
| control-identity clippy `--all-targets -D warnings` | 0 | Finished |
| identity-binding-api lib | 0 | 3 passed, including the Git ref CAS |
| identity-binding-api clippy `--all-targets -D warnings` | 0 | Finished |
| identity-binding-api `--bin` build | 0 | |
| a370 `tests/live` `OO_A370_LIVE=1` | 0 | 1 passed, 1.86s |
| `tests/binding_loop` `OO_A402_LIVE=1` | 0 | 1 passed, 2.66s |

`a402-proof.json` records install, preview with no write, approve, idempotent replay, idempotency conflict, stale session, logout, reviewer current, reviewer forbidden, unchanged grant, self-grant and self-revoke elevation, cross-project forbidden, bad CSRF, stale revision, caller JSON malformed, bootstrap frozen by a second admin, auditor and business_owner current, restart, one winner of two concurrent approves, and zero `oo_r_*` roles.

This run does not accept full SP02, a customer IdP, or a release.
