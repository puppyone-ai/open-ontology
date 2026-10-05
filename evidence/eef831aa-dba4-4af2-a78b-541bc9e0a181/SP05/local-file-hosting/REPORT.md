# REPORT — a181 local file hosting

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a181`. Slot C. One Rust builder, no PostgreSQL.

Source commit `755fb84acc7f80e5ba2fba1c2b80412dab7fab26` on parent `72b5136ba23d134916425132523383947046aeff`. The commit contains only:

- `crates/oo-control/src/lib.rs`
- `crates/oo-control/src/file_hosting.rs`
- `crates/oo-control/tests/file_hosting.rs`
- `crates/oo-control/Cargo.toml`
- `contracts/control/local-file-hosting-v1.md`
- `Cargo.lock`

File hashes are in `SHA256SUMS`. The dependency table is in `dependency-delta.md`. The behavior contract is `contracts/control/local-file-hosting-v1.md`.

## What the slice does

`oo_control` now stores multipart upload sessions on `object_store` 0.14.2, feature `fs` only. `FileHost::open_local` binds a deployment directory to `LocalFileSystem` with fsync. `FileHost::new` takes `Arc<dyn ObjectStore>`. Upload methods take an `AdmissionContext`, session ids, object keys, and bytes. They do not take a caller filesystem path.

The host generates the session id and the key `uploads/<session>/<sha256>/original`. Parts are ordinary objects, so a new `FileHost` on the same directory can resume. `complete` calls `put_multipart_opts` only after the stored part bytes match the expected sha256 and size, then re-hashes the published object before writing the manifest. A checksum or size miss deletes that session's owned staging keys and does not create the object key. A foreign name under the part prefix, and a file outside the store root, are left in place.

Project and principal are the authorization check. A known key for another context returns `Denied`. A missing key or a wrong digest returns `NotFound`. Grant evidence is stored and is not part of the idempotency key. The same project and principal can resume with a different grant string. The manifest keeps the first grant. The manifest carries a lineage digest over the documented domain.

Plan, apply, secrets, RBAC, HTTP, and S3 are not implemented. The architecture check printed `architecture check: pass`. `policy.rs` was not edited.

## Linux smokes

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `os=linux`, `arch=arm64`. Toolchain `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. Container `oo-a181-impl-control` on the final run: privileged false, CapDrop ALL, no-new-privileges, network `oo-a181-eef831aa-net`, no host network, no published ports, memory 8 GiB, `/tmp` tmpfs. `HOME=/tmp`. Private volumes `oo-a181-eef831aa-cargo` and `oo-a181-eef831aa-target`. No PostgreSQL.

Pre-edit control smoke, before any product edit, archive of `03512f91324ac33e772e9702fe806e8e04037ec4`: `pre-edit/docker-run.exit` is 0 and `pre-edit/phases/worst.exit` is 0. Fetch, `oo-control` tests, and Clippy were locked and offline.

Final implementation smoke, archive of `72b5136b` plus the owned overlay: `impl/docker-run.exit` is 0. Phase exits are 0 for rustfmt component, clippy component, fmt, fetch, lock delta, cargo tree, test, clippy, arch, and worst. Elapsed 34 seconds on that run because the private target volume already held the dependency builds. `cargo test -p oo-control --test file_hosting --lib` reported 0 library tests and 6 integration tests passed. Clippy `--all-targets -D warnings` finished clean. `oo-arch-check --manifest /src/Cargo.toml` printed `architecture check: pass`.

The six tests open a real `LocalFileSystem` under a private scratch directory:

- `multipart_reopen_reads_manifest_and_lineage`
- `conflicting_part_and_config_do_not_overwrite`
- `two_contexts_deny_a_known_key`
- `checksum_and_size_failure_leave_no_visible_object`
- `abort_releases_quota_and_leaves_foreign_bytes`
- `limits_and_names_are_refused_before_write`

## Failed attempts kept

| Directory | Exit | What stopped |
|---|---|---|
| `impl/attempt1` | 4 | Lock checker treated itertools 0.15.0 as a forbidden bump. Fetch had added it beside the existing itertools rows. |
| `impl/attempt2` | 101 | `get` and `delete` need `ObjectStoreExt` in scope. `object_store` 0.14.2 itself compiled, including nix. |
| `impl/attempt3` | 101 | The integration test could not see `idempotency_key`, `MAX_OBJECT_BYTES`, or `MAX_PART_COUNT`. The library compiled. |
| `impl/attempt4` | 101 | The checksum fixture is 15 bytes and the test required 16. Five other tests passed. |
| `impl/attempt5` | 101 | Tests passed 6/6. Clippy denied `collapsible_if`. |
| `impl/attempt6` | 101 | Tests passed 6/6. Clippy denied `too_many_arguments` on the test lineage helper. |

Raw logs, phase exits, the image inspect, host config, and the cargo tree are under `pre-edit/` and `impl/`.

## Still open

S3 and MinIO, the 5 MiB part minimum, reaping `dest#N` files, a crash mid-cleanup, download, TTL, an auth proxy, HTTP, MIME sniffing, formula-safe output, retention delete, presigned URLs, a linearizable quota, grant signature checks, and the rest of REQ-SRC-05 after `stored`. SP05-W01 and SP05-W06 are not closed. The ledger was not edited. This commit is not a release.
