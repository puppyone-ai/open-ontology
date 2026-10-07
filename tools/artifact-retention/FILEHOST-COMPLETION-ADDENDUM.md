# FileHost retention completion addendum

Contract `oo.control.local-file-hosting/v1` completion addendum.
File `FILEHOST-COMPLETION-ADDENDUM.md`.
Frozen before the source change that implements it.
It does not close SP05-W06, AT-FILE-05, or AT-SEC-03. `parent_complete` stays false.
It does not add a countable unit.

This addendum supersedes two statements in `FILEHOST-DELETE-CONTRACT.md` and `CONTRACT.md`:

- completed-object quota is released before the object bytes are deleted, including `stop_after_quota_release`
- an open `abort` cannot prove part absence, and registered `dest#N` staging is left in place

The other frozen statements stay. `abort` of a completed session is still `Closed` and still deletes nothing. `FileHost::new` still returns `Unsupported` before store IO for every delete, observation, and abort call. Workspace snapshot TTL stays 24 hours and default off. Uploaded source TTL stays 180 days. Session output registration still uses the upload key `uploads/<session>/<sha256>/original`. This crate still does not create `sessions/<id>/outputs/`. There is still no object version, no HTTP URL, no authenticated download proxy, and no formula-safe CSV or Excel writer.

## Caller proof

Every delete and every absence observation takes the admission context plus the manifest tuple the host already returned: object key, object sha256, size, and lineage digest. An open observation takes the admission context and the session id `begin` returned. There is no filesystem path argument, no caller quota path, and no scan of an arbitrary directory supplied by the caller.

Project, principal, and grant are compared on the delete path and on the open-absence path. A project or principal mismatch is `Denied`. A grant mismatch is `Denied`. A key, digest, size, or lineage mismatch is `NotFound`. A session record that is present and disagrees with the manifest is `Conflict`. The quota body, when present, must be the decimal size with no leading zeros. Any other body is `Conflict` and is not overwritten.

Download admission is unchanged. Every pull compares the current declared tuple. Project and principal are the host check. Role and policy are the inventory row. That check is not an external certificate. `gateway_url_revocation` stays `not_implemented`. `bearer_url` stays `none`.

## Quota stays until the bytes are absent

Project quota stays the sum of `meta/quota/<project>/<session>` bodies. This addendum does not redefine that sum as a disk-byte counter.

A completed object keeps its reservation until the object bytes, the sidecars, the five-digit parts, and the platform staging files named below are durably absent. Only then is the one quota key removed. A new `begin` while that key still exists is admitted only when the reservation sum, including this key, fits `max_project_bytes`. The window where the quota key is already gone and the object file is still present is not a state this API creates.

`delete_completed` no longer takes `stop_after_quota_release`. The boolean is `stop_after_data_absent`.

```text
delete_completed(
    ctx,
    object_key,
    object_digest,
    size,
    lineage_digest,
    stop_after_data_absent,
) -> Result<Option<ObjectManifest>, FileHostError>
```

`true` returns `Ok(None)` only after `data_absent` is durable. The quota key is still present and `release_count` is still 0. `false` continues through the single quota release and `verified`. Neither value releases quota while the bytes, sidecars, parts, or platform staging are still present. There is no public flag that stops after a quota release taken before those bytes are gone.

`Ok(None)` means data absence is durable and the reservation is still held. `Ok(Some(_))` means absence was verified after the single release. Any `Err` is not completion and does not release quota unless a legacy tombstone had already released it, as below.

## Phases

One tombstone, `meta/tombstone/<session>`, at most 4096 bytes. New bodies add one line, `order bytes_then_quota`, after `release_count`. The other lines stay the frozen record.

```text
phase data_deleting|data_absent|quota_released|verified
release_count 0|1
order bytes_then_quota
```

| Phase | release_count | Meaning |
|---|---|---|
| `data_deleting` | 0 | Owned bytes are being removed. The quota key is still present. |
| `data_absent` | 0 | Bytes, sidecars, five-digit parts, and platform staging are absent. The quota key is still present. |
| `quota_released` | 1 | That one key was removed after `data_absent`. |
| `verified` | 1 | The same absence check passed again after the release. |

Forward only: absent to `data_deleting`, `data_deleting` to `data_absent`, `data_absent` to `quota_released`, `quota_released` to `verified`. A later call does not write an earlier phase and does not raise `release_count` past 1.

`data_absent` is written only when the local node and the store agree that the object, the manifest, the session, the idempotency pointer, and every five-digit part are absent, and the platform staging files for those keys are absent. A symlink, a non-file, a local/store mismatch, or a read error is not absence. The call returns `Corrupt` or `Store` and leaves the quota key in place.

A crash or a forced stop before that absence keeps `release_count` 0 and keeps the quota key. Restart continues the byte deletion and does not release early.

A crash after the absence is durable and before `quota_released` is written leaves `data_absent` and `release_count` 0. Restart deletes that same quota key once. `NotFound` on that delete is the success of the delete call. Restart then sets `release_count` 1. It does not free a second reservation.

`completed_quota_release_count` is still 0 or 1. A missing tombstone is 0. It does not prove absence.

## Legacy `quota_released` tombstones

A tombstone in the previous format, with phase `intent` or `quota_released` and no `order` line, is legacy. It is not rewritten into `data_absent`.

- Phase `quota_released` with `release_count` 1, or phase `intent` whose quota key is already gone, means the reservation was removed before the bytes. Restart deletes the owned bytes, sidecars, parts, and platform staging. It does not write the quota key back. It does not delete or overwrite any other quota key. If this session's quota key is present with a different body, the call is `Conflict` and writes nothing. If it is present with this size while `release_count` is already 1, it is left in place. Absence is `Ok(true)` only after the bytes are actually gone and the phase is then `verified`. `release_count` stays 1.
- Phase `intent` with `release_count` 0 and the quota key still present is not a release. Restart moves forward to `data_deleting` and follows the new order.

The rewritten legacy body uses `order legacy_reconcile`. That marker is not a claim that the bytes were already absent.

## What may be removed

Only keys taken from the manifest or the session record, under the deployment lock:

| Key | When |
|---|---|
| five-digit `staging/<session>/part/<nnnnn>` | before the quota key |
| platform staging `{those keys}#{n}` and `{object,manifest,session,pointer}#{n}` | before the quota key |
| `uploads/<session>/<sha256>/original` | before the quota key |
| `meta/manifest/<session>`, `meta/session/<session>`, the idempotency pointer | before the quota key |
| `meta/quota/<project>/<session>` | only after `data_absent`, and only that one key |

`<n>` is a decimal with no leading zero. The object store rejects a key whose final component ends in `#` plus digits, and its list skips those files. The host therefore unlinks only a regular file at that computed path, under the canonical deployment root, after `symlink_metadata` says it is a regular file. The reaper does not unlink it. A symlink is `Corrupt` and is not removed. A read error is `Store` and is not absence.

Names that are not those keys stay. `notes`, a literal `dest#1` that is not `{platform file}#{n}`, and a file outside the deployment root stay. An empty directory stays.

## Open abort

`abort` of an open session still returns `Result<(), FileHostError>`. A second call on a removed session is still `NotFound`. A completed session is still `Closed`.

The locked order for an open session is: delete the five-digit parts, reap that session's platform staging, and only then delete the quota key, the idempotency pointer, and the session. If a part or a platform staging file is still present, or any of those checks is a symlink or a read error, abort returns the error and keeps the quota key.

It then writes `meta/open-absence/<session>`:

```text
oo.control.local-file-hosting/v1 open-absence
session <32 hex>
project <project id>
principal <principal id>
grant_hex <lowercase hex>
object_key <platform key>
expected_digest <64 lowercase hex>
expected_size <decimal>
```

```text
observe_open_absence(ctx, session_id) -> Result<bool, FileHostError>
```

`Ok(true)` is the only open-absence proof. It requires the receipt, a matching project, principal, and grant, no five-digit part, and no platform staging file. A symlink or a non-file on those keys is `Corrupt`. A store or directory read error is `Store`. A foreign name in the same directory does not make the result `Ok(false)` and is not deleted. No receipt is `NotFound`. `read_object` `NotFound` is not this proof.

`FileHost::new` returns `Unsupported` for `observe_open_absence` before store IO.

## Reaper

The reaper calls the host. It does not unlink store files and does not edit quota files.

`--pause-after` is `pre_delete`, `metadata_api`, `data_deleting`, `data_absent`, or `none`. `quota_released` is not a pause value. `data_deleting` checkpoints and returns before `delete_completed`, so the quota key and the bytes are still present. `data_absent` calls `delete_completed` with `stop_after_data_absent` true, checkpoints, and returns while the reservation is still held. Exit 75 is that pause. The process has not printed success.

A published row succeeds only when `delete_completed` returns `Ok(Some)`, `observe_completed_absence` returns `Ok(true)`, and `completed_quota_release_count` is 1. An open row succeeds only when `abort` returns `Ok` and `observe_open_absence` returns `Ok(true)`. `deletion_success` is true only when every expired row reached one of those two proofs. `parent_complete` stays false.

## Seams that stay missing

`required_integration` keeps these names. They are not renamed.

| Seam | Why it stays |
|---|---|
| `filehost_object_version` | The manifest has no version. An empty local version is not recorded. |
| `filehost_admission_role_and_policy` | The host compares project, principal, and, on delete, grant. Role and policy stay on the inventory row. The declared tuple is not an external certificate. |
| `gateway_authenticated_url_revocation` | No HTTP transport and no authenticated download URL. |
| `formula_safe_csv_excel` | No CSV or Excel writer. |
| `sessions_outputs_prefix` | Keys stay `uploads/<session>/<sha256>/original`. The output prefix is not implemented. |

`filehost_part_absence_observation` and `filehost_dest_staging_reap` are the two gaps this addendum specifies. They leave `required_integration` only when the source matches this file. Removing them is not acceptance of the parent card.
