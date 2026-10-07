# FINDINGS — a323 completed-object delete

Verdict: `parent_complete` is false on product commit `c27525a2a7c31dfbe9cb64b183312c7dab354511`. Proof-3 exited 0 and is the focused measurement. Exit 0 is not acceptance of SP05-W06, AT-FILE-05, AT-SEC-03, or the ledger.

`crates/oo-control/src/file_hosting.rs` blob `aa16be987070522105b7b4079ed00ef89899b7f3`, sha256 `8804b4eea7463c4853ca159e795eb6edcd2bf53d9bcbb86ece8af98752442627`. The frozen addendum blob is `cb8de1af2712005b481ac2a0a792a9cff0ab9014`. Root `Cargo.lock` blob `2990f901ea98a7b465f462cce8742bc5c51c13aa`, sha256 `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Child lock sha256 `41ae41f864e1dc1a6089b3ef70a649949744273fe887056b4fa2cb57828cfc70`.

`lib.rs` was not edited, so the public methods use types already exported from `oo_control`: `FileHost`, `FileHostError`, `AdmissionContext`, `ObjectKey`, `ObjectManifest`, plus `bool`, `u64`, and `[u8; 32]`. `delete_completed` returns `Result<Option<ObjectManifest>, FileHostError>`. `observe_completed_absence` returns `Result<bool, FileHostError>`. `completed_quota_release_count` returns `Result<u8, FileHostError>` and takes `(ctx, object_key)` only.

`FileHost::new` still has no deployment lock. The three new calls return `Unsupported` before store IO. `abort` of a completed session still returns `Closed` and deletes nothing.

## Authorization

The object key must embed the digest argument. A mismatch is `NotFound` and writes nothing. The manifest size, digest, lineage, project, principal, and grant must match. Size 0 is `NotFound`. A project or principal mismatch on a matching digest is `Denied`. A grant mismatch on a matching project and principal is `Denied`. A session record, when present, must agree or the call is `Conflict`. A quota body, when present, must be the decimal size without a leading zero. An idempotency pointer, when present, must name this session. The call does not search for another quota key and does not accept a guessed path.

## Tombstone and one release

The tombstone key is `meta/tombstone/<session>`, capped at 4096 bytes. Phases are `intent` with `release_count` 0, `quota_released` with `release_count` 1, and `verified` with `release_count` 1. Any other pair is `Corrupt`. The record is overwritten only forward. `release_count` does not move past 1.

Quota release deletes `meta/quota/<project>/<session>`. A missing key is success for that delete, so a retry frees the reservation once. After the quota phase, `stop_after_quota_release` returns `Ok(None)` while the object, manifest, session, and pointer are still present. That return is not completion. A later call continues from the tombstone and does not delete a second quota key.

`Ok(Some)` means the five-digit parts, object, manifest, session, and matching idempotency pointer were deleted, the absence check passed, and the tombstone is `verified`. `Err` is not completion. A verified tombstone whose deleted key reappears is `Conflict` and does not release quota again.

## Absence

`observe_completed_absence` takes the deployment lock, writes nothing, and unlinks nothing. `Ok(true)` requires a verified tombstone, `release_count` 1, the same key, digest, size, lineage, project, principal, and grant, and local-plus-store agreement that the object, the manifest, and every five-digit part are absent. A store or stat error stays `Store` or `Corrupt`. It is not absence. `read_object` `NotFound` is not this proof.

No tombstone and no manifest is `NotFound`. A live manifest that still matches, with no tombstone, is `Ok(false)`. Intent or `quota_released` with keys already gone is `Conflict`, so the reaper calls `delete_completed` again to record `verified`. Non-five-digit names, including `dest#1` and `notes`, are not deleted and do not flip `Ok(true)` to false.

A symlink or non-regular file on a checked key is `Corrupt`. Parents are not followed through a symlink. The leaf is not unlinked.

## Readers

`open_download` and `next_chunk` do not take the deployment lock and do not change quota. A pull that already returned `Ok(None)` with `is_complete` true stays complete. Bytes already returned are not recalled. If the next read is the end-of-file check and the head is missing, the error is `Corrupt` and `is_complete` stays false. If the offset is still below the size and the next `get` fails, the error is `Store` and `is_complete` stays false.

## Reaper

The reaper calls `delete_completed`, then `observe_completed_absence`, then `completed_quota_release_count`. It does not unlink store files and does not edit quota bytes itself. Row success requires `Ok(Some)` from the finishing delete, `Ok(true)` from observation, and count 1. Status is `deleted`. `deletion_success` is true only when every expired outcome in that reap has `deleted` and `absence_verified`. An expired open session that is still open stays `aborted_parts_unverified`, so a mixed batch keeps `deletion_success` false. An idle reap is exit 0 with `deletion_success` false.

A published row whose `open_download` is `NotFound` consults the absence API. `Ok(true)` and count 1 is an idempotent success and does not delete quota again. `NotFound` with no tombstone stays `observed_not_found_unverified` and seam `filehost_part_absence_observation`.

The checkpoint file is a cursor. Its body keeps `absence_verified false` and `deletion_success false`. It is not a success receipt. Restart calls the host again. The tombstone decides the phase.

`--pause-after quota_released` returns before the success line is printed. Exit 75 happens after `control/continue` appears. The kill test sends SIGKILL at the checkpoint and asserts the captured stdout has no `"deletion_success":true`.

`parent_complete` is set false on the idle report, the paused report, and the finished report. The JSON field is that boolean. Exit 0 means idle or `deletion_success`. Exit 2 means a reap ran and deletion is not verified. Exit 1 is a bad root, a corrupt record, or a bad argument.

## Operator and clocks

`Deployment::create` takes the operator string. `open` reads `owner.txt` and requires the directory's final component to equal that UUID. The grammar is 8-4-4-4-12 lowercase hex. The constant `OWNER_UUID` is gone. An a311 directory still opens when its name and `owner.txt` carry `eef831aa-dba4-4af2-a78b-541bc9e0a311`.

Caps are unchanged. Uploaded source and transcript are `180 * 86400` seconds. Session output is `30 * 86400`. Workspace is `86400` only when `explicit_workspace` is set. Download permit cap is 300 seconds. The CLI clock is `Now::Production`. Tests use `Now::Explicit`. There is no `--now` flag.

## Still open

Open-abort part absence, `dest#N` reap, an object version, host admission of role and policy, authenticated gateway URL revocation, formula-safe CSV/Excel, and `sessions/<id>/outputs/` stay in `required_integration()`. This crate does not revoke an HTTP URL. `bearer_url` is `none`. `gateway_url_revocation` is `not_implemented`.
