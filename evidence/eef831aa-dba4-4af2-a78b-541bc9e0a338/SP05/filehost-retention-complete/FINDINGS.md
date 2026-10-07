# FINDINGS — a338 retention completion

Verdict: `parent_complete` is false on product commit `def1f7eea53794449d3975b1987d700727cb7d60`. Post-edit-4 exited 0 and is the focused measurement. Exit 0 is not acceptance of SP05-W06, AT-FILE-05, AT-SEC-03, or the ledger.

`crates/oo-control/src/file_hosting.rs` blob `8943ac612ca39be64440d8301dd12b48b5d39976`, sha256 `f37377cb0bc359c84c2efb1bce55a69d429770e334bec9544906daca49e6e5ce`, 4804 lines, zero occurrences of `unsafe`. The frozen addendum blob is `3a9de3efdca24ec03b3b4e197a1119f2afa8a2cd`, sha256 `5e09df2722f3590d3761540a9f7400e2dce9658008571065592537cd0f0f8cb1`, and `git diff` against `9cdbc625` is empty. Root `Cargo.lock` blob `45d24ef0a15b2db24fc0b9dd02826cd1056ecae8`, sha256 `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`, same blob at the baseline. Child lock sha256 `41ae41f864e1dc1a6089b3ef70a649949744273fe887056b4fa2cb57828cfc70`.

`lib.rs` was not edited. New methods sit on the already exported `FileHost` and return `bool` or types `oo_control` already exports. `delete_completed` still returns `Result<Option<ObjectManifest>, FileHostError>`. The sixth argument is `stop_after_data_absent: bool`. `abort` still returns `Result<(), FileHostError>`. `observe_open_absence` returns `Result<bool, FileHostError>` and takes `(ctx, session_id)`. `observe_completed_absence` and `completed_quota_release_count` keep their previous signatures.

`FileHost::new` still has no deployment lock. Delete, both observations, and `abort` return `Unsupported` before store IO. `abort` of a completed session still returns `Closed` and deletes nothing.

The lower half of `FILEHOST-DELETE-CONTRACT.md` still prints the historical signature `stop_after_quota_release`. The banner at the top says the completion addendum wins, and the implementation argument is `stop_after_data_absent`. `--pause-after quota_released` is rejected as `Admission`. An on-disk checkpoint named `quota_released` remains a legacy restart cursor.

## Quota stays until the bytes are gone

The enforced sum is still the bodies of `meta/quota/<project>/<32-hex-session>`. `meta/quota/<project>/notes` is not a reservation. There is no disk-occupancy counter. A reservation is deleted only after the object, manifest, session, matching idempotency pointer, five-digit parts, and platform `{basename}#{n}` staging for those keys are absent.

`HostLimits::try_new(64, 64, 10, 64)` still admits one 64-byte reservation. While that key remains, a second 64-byte begin is `Quota`, including at phase `data_absent` with `release_count` 0. After `verified` and count 1 the key is gone and a begin of that size succeeds. The formula was not redefined as a sum that ignores the held key.

`stop_after_data_absent` true returns `Ok(None)` only when absence is durable and the quota key is still present. If the key is already gone, the call records `quota_released` with count 1 and `verified`, then returns `Ok(Some)`. There is no public flag that releases quota while the bytes are still present.

## Phases

Tombstone key `meta/tombstone/<session>`, body at most 4096 bytes. New bodies carry `order bytes_then_quota`. A reconciled legacy body carries `order legacy_reconcile`. A body with no order line decodes as `Legacy`. An unknown extra line is `Corrupt`.

Checked forward pairs include: no tombstone to `data_deleting` count 0 `bytes_then_quota`; `data_deleting` to `data_absent`, both count 0; `data_absent` count 0 to `quota_released` count 1; that row to `verified` count 1. Count never decreases and never exceeds 1. `NotFound` on the already-deleted quota key is success of that delete, so a restart releases once.

A legacy `intent` with the quota key still present rewrites to `data_deleting` / `bytes_then_quota` / count 0. A legacy `quota_released` or `intent` whose key is already gone rewrites to `quota_released` / `legacy_reconcile` / count 1. That path does not write the reservation back and does not overwrite a foreign body. A present key whose body is not the decimal size is `Conflict` before any tombstone write. A verified `bytes_then_quota` row whose quota key reappears is `Conflict`. A verified legacy row may still show a matching leftover key; the call does not delete it again and does not treat it as a second release.

`observe_completed_absence` writes nothing. `Ok(true)` requires `verified`, count 1, the declared tuple, and owned absence. For `bytes_then_quota`, the quota key must also be absent. `data_absent` with the bytes still present is `Corrupt`. `data_absent` with the bytes gone is `Ok(false)`. A fresh manifest with no tombstone is `Ok(false)`. No tombstone and no manifest is `NotFound`.

## Idempotency slot after verify

The pointer key is `meta/idempotency/<project>/<principal>/<64-hex>`, not session-scoped. After a verified delete, `begin` of the same upload config creates a new session and writes that new 32-hex id into the same pointer. `owned_absent` treats the pointer as residue only while its body is still this session id. A later 32-hex body is allowed only when the tombstone is already `verified`. Any other body is `Conflict`. A verified restart does not delete the reused pointer. Post-edit-3 failed because absence required the pointer key itself to be missing, so the repeated reap exited 2. Post-edit-4 is the run that includes this rule.

## Open absence and platform staging

`cleanup_open` refuses a five-digit symlink or a platform-staging symlink before any delete, and it keeps the quota. Regular platform parts are deleted through the object store. Platform staging files whose names are exactly `{basename}#{canonical digits}` are unlinked after `symlink_metadata`. The suffix has no leading zero, is all digits, and is in `1..=10000`. A symlink or a non-file is `Corrupt` and is not unlinked. A directory read error is `Store`. `NotFound` between `read_dir` and `stat` is already gone. Foreign names (`notes`, a literal `dest#1`, `original#01`, non-utf8) are skipped. The reaper does not `remove_file` store paths and does not edit quota files.

The open-absence receipt `meta/open-absence/<session>` is written while the quota key is still held, and only after the part directory is clear. A different existing body is `Conflict` and does not delete quota. The quota key, the pointer, and the session are deleted after the receipt. `observe_open_absence` writes nothing. `Ok(true)` requires that receipt, the same project, principal, and grant, and no five-digit part or platform staging file. No receipt is `NotFound`. `read_object` `NotFound` is not this proof.

Addendum line 132 says an open row succeeds only when `abort` returns `Ok` and `observe_open_absence` returns `Ok(true)`. `finish_abort` also treats `Err(NotFound)` plus `Ok(true)` as status `aborted`, seam `none`, `deleted` true, `absence_verified` true. That arm covers a later call after the session key is already gone and the receipt remains. The addendum was not rewritten. The Linux lib test proves the first `abort` is `Ok`, observation is `Ok(true)`, and a second `abort` is `NotFound` while observation stays `Ok(true)`. There is no separate SIGKILL test of the window between the receipt write and the session delete.

`complete` size or checksum failure still calls `cleanup_open`. A symlink that blocks that cleanup returns `Corrupt` and keeps the quota.

## Caller proof and the five seams

Admission of a delete uses `AdmissionContext` plus the manifest tuple or the session from `begin`. Project, principal, or grant mismatch is `Denied`. Key, digest, size, or lineage mismatch is `NotFound`. A session record that disagrees with the manifest is `Conflict`. Download admission still compares the current project and principal on the host. Role and policy stay on the inventory row. That check is not an external certificate.

Session output registration still uses `uploads/<session>/<sha256>/original`. This crate does not create `sessions/<id>/outputs/`. There is still no object version, no HTTP URL, and no authenticated download proxy. `gateway_url_revocation` stays `not_implemented`. `bearer_url` stays `none`. Workspace snapshot TTL stays 24 hours and default off. Uploaded source stays 180 days.

`required_integration()` returns exactly:

- `filehost_object_version`
- `filehost_admission_role_and_policy`
- `gateway_authenticated_url_revocation`
- `formula_safe_csv_excel`
- `sessions_outputs_prefix`

`filehost_part_absence_observation` and `filehost_dest_staging_reap` are absent from that list because post-edit-4 proved them. The other five stay under those names. Plan SP05-W06 still requires the authenticated proxy, versioned unified delete, formula-safe CSV/Excel, AT-FILE-05, and AT-SEC-03. Those items are not done here.
