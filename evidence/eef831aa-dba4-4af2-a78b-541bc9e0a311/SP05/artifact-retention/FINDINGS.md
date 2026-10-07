# FINDINGS — a311 local artifact retention

Verdict: `parent_complete` is false on fixed commit `cab5e8e219d1924070df7336f5ab5d4ce7cf40dc`. Proof-6 exited 0 and is the focused measurement. Exit 0 is not acceptance of SP05-W06, AT-FILE-05, AT-SEC-03, or the ledger.

`crates/oo-control/src/file_hosting.rs` blob `8c32a4feaf69d9a445a5839ba2a01982c994936b`, sha256 `05c9d4d7e2888c40d9de848021f3452968671ae0a3ea04ca58a13f147c49b38b`. Root `Cargo.lock` blob `2990f901ea98a7b465f462cce8742bc5c51c13aa`, sha256 `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Child lock sha256 `41ae41f864e1dc1a6089b3ef70a649949744273fe887056b4fa2cb57828cfc70`.

The crate calls `FileHost::open_local`, `begin`, `put_part`, `complete`, `abort`, `read_manifest`, `read_object`, and `open_download`. `ObjectDownload` is reached through those calls. `delete_key`, `delete_parts`, and `list_prefix` are private on the host. `FileHost::new` returns `Unsupported` on mutating calls and takes no deployment lock, so this crate does not use it to delete.

## Inventory

`Deployment::create` accepts one real directory whose final component is `eef831aa-dba4-4af2-a78b-541bc9e0a311`. A symlink, or any other final component, is refused. `filehost/` inside that directory is the only path passed to `open_local`. The platform lock beside it stays with FileHost.

Published registration stores the manifest `read_manifest` returns, plus one caller role and one policy id. Those two strings are not in the host manifest. The grant is stored as hex. Open registration calls `begin` inside the crate and stores the session that call returns. The caller does not supply a session id, a manifest document, a version, or a filesystem path.

`version` is `absent` with reason `filehost_manifest_has_no_version`. The local backend's empty version is not recorded as an immutability token. `ObjectKey::parse` accepts `uploads/<32 hex>/<64 hex>/original`. A `sessions/<id>/outputs/` path is not a registration.

## Clock and classes

`Now::Production` reads `SystemTime::now`. The CLI reap path has no timestamp flag. Caps, in seconds: uploaded source and transcript `180 * 86400`, session output `30 * 86400`, workspace snapshot `86400` and only when `explicit_workspace` is true, quarantine `7 * 86400`, eval `365 * 86400`. Knowledge packs and audit anchors are not deletion classes. Download permit cap is 300 seconds. A zero TTL or a TTL above the cap is refused. The stored TTL is the requested value.

## Download tuple

Project and principal are compared by `FileHost::open_download`. Role and policy are compared to the inventory row, because the host does not store them. A project or principal mismatch is the host `Denied`. A role or policy mismatch runs the host check when project and principal match, drops the download before `next_chunk`, and returns `TupleMismatch`. An expired row is refused before bytes are returned. Each decision records `gateway_url_revocation=not_implemented` and `bearer_url=none`. This crate does not speak HTTP.

## Reaper

The expired set is inventory rows with `expiry_unix_ms <= now`. Active rows stay. The reaper does not scan the store for unregistered keys.

Before `abort` it fsyncs a pre-delete audit and a checkpoint. The audit holds at most 64 SHA-256 line records. Past that bound the call returns `Limit` and does not delete.

Observation uses `open_download` and bounded `next_chunk` reads. The running hash and `is_complete` must match the manifest, and the manifest must match the inventory row. Drift, a symlink, a corrupt body, or a store error does not call `abort`. `NotFound` alone is not absence and is not success.

The only deletion call is `abort`.

- A completed session returns `FileHostError::Closed`. The object, manifest, session record, idempotency pointer, and quota reservation stay. Status `unsupported_completed_object_delete`. `deleted` false. `absence_verified` false. A later `read_object` is the host result. The published-reap test asserts that result is `Ok`.
- An open session can return `Ok` from `abort`. There is no public list or stat of `staging/<session>/part/<nnnnn>`. Status `aborted_parts_unverified`. `deletion_success` stays false. The live test may see the part path gone on a direct `exists` check. That observation is outside the public API and is not promoted to absence.
- `dest#N` staging is outside `abort`.

`deletion_success` requires every expired row to be deleted through a host API and then proved absent, including manifest sidecars and multipart parts, with no store error treated as absence. That condition is unreachable on this host API. `parent_complete` is hard-coded false on every report, including the idle report.

A checkpoint at `pre_delete`, `metadata_api_calling`, or `metadata_api_done` makes the next reap `restarted_from_partial`. The CLI can wait after `metadata_api_done` (`control/waiting`, poll `control/continue` every 200ms) so a test can `SIGKILL` the process while it is still alive. The restart stays partial until absence is verified. The object step does not unlink bytes.

Quota and replay stay on the host. The published test fills `project_a` with `HostLimits::try_new(64, 64, 10, 64)`, reaps, and still sees the same session on replay and `Quota` on a second `begin`. `project_b`'s unregistered object is still readable after the reap.

## Seams still required

| Seam | Host fact |
|---|---|
| `filehost_completed_object_delete` | No public call deletes a completed object and its manifest under the deployment lock. |
| `filehost_quota_release_on_completed_delete` | Completing an object keeps the reservation. This crate does not edit quota objects. |
| `filehost_part_absence_observation` | No public call lists or stats part keys after `abort`. |
| `filehost_dest_staging_reap` | `dest#N` leftovers are outside `abort`. |
| `filehost_object_version` | The receipt has no version field. |
| `filehost_admission_role_and_policy` | Host admission compares project and principal. |
| `gateway_authenticated_url_revocation` | No HTTP transport and no authenticated download URL. |
| `formula_safe_csv_excel` | No CSV or Excel writer in this crate. |
| `sessions_outputs_prefix` | FileHost keys stay `uploads/<session>/<sha256>/original`. |

## Proof failures that stayed on disk

proof-1, `docker-run.exit` 126. `rustc` wrote `/tmp/lock_from_baseline` and exec failed because the tmpfs is `noexec`. Later proofs compile that oracle to `/target/lock_from_baseline`.

proof-2, `docker-run.exit` 101. Walk of 198 packages / 194 registry pins failed `cargo metadata --locked` (exit 101). The fallback superset plus one `cargo metadata --offline` passed the checksum gate (`check_ok registry_pins=185 packages=189`, `phases/checksum-gate.exit` 0). Final locked metadata did not rewrite the child lock. Compile errors are `E0624` on `pub(crate) fn control_dir` from `src/main.rs`, and deprecated `as_slice` at the digest compares then in `permit.rs` and `reaper.rs`. The sources proof-6 compiled use `pub fn control_dir` and `AsRef::<[u8]>::as_ref`.

proof-3, `docker-run.exit` 101. `env!("CARGO_BIN_EXE_artifact_retention")` is unset. Cargo 1.94 sets `CARGO_BIN_EXE_artifact-retention`. The test now uses `option_env!` for the hyphen form, then the underscore form, and `expect`s if both are absent. `Deployment` derives `Debug`. `scratch` returns the parent and the root as a pair.

proof-4, tests green, `phases/clippy.exit` 101, six errors: two `single_match`, one `while_let_loop`, `large_enum_variant` on private `Seen`, `collapsible_if` on the audit scan, `manual_is_multiple_of` in `text.rs`.

proof-5, tests green, `phases/clippy.exit` 101, one `collapsible_if` at the kill-test checkpoint read. `linux/proof-5/launch.sh` lines 39–43 leave a quote open, and `recipe-origin.txt` contains the leaked echo text. The console is still the container result. proof-6 was launched from a quoted script, not from that broken file.

proof-6 is the green run. Details are in `REPORT.md`. `deletion_success` in the published, drift, open-abort, and kill tests stays false. The CLI stdout contains `"deletion_success":false` and `"parent_complete":false`.

## Bounds of this measurement

The public host API cannot delete a completed object, release its quota, or prove part absence. This crate returns those seams as unsupported. It does not unlink store paths, follow a symlink to clean it, or rewrite quota files. Authenticated gateway URL revocation is not implemented. Formula-safe CSV and Excel are parent scope and are not in this crate. The 24 hour workspace class is opt-in. An idle reap exits 0 with `deletion_success` false. A reap that runs and cannot verify absence exits 2. A paused checkpoint exits 75.
