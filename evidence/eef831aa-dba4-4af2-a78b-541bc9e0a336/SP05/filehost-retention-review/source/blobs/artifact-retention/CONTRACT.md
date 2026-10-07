# artifact-retention-v1

Contract `oo.artifact-retention/v1`. Crate `artifact-retention`.

This crate keeps a durable inventory of objects a real `oo_control::FileHost` has already issued, and it decides expiry against that inventory. It is not a second object store. It does not invent blobs, manifests, or object versions. It does not close SP05-W06, AT-FILE-05, or AT-SEC-03.

The host API is `oo.control.local-file-hosting/v1`. `abort` of a completed session still returns `Closed` and deletes nothing. Completed-object deletion is `FileHost::delete_completed`, frozen in `FILEHOST-DELETE-CONTRACT.md` before this source change. Public calls used here are `open_local`, `begin`, `put_part`, `complete`, `abort`, `read_manifest`, `read_object`, `open_download`, `delete_completed`, `observe_completed_absence`, and `completed_quota_release_count`. `ObjectDownload` is used through those calls. There is no public list and no public head of an arbitrary key.

## Deployment

`Deployment::create(root, limits, operator)` takes an explicit operator UUID. The grammar is 8-4-4-4-12 lowercase hex. The directory's final component must be that UUID. A symlink is refused. A path whose name is not `operator`, including a public user path, is refused. `..` is refused. The process writes `owner.txt` with that UUID and the `HostLimits`, then `fsync`s the file and the directory. `Deployment::open` reads `owner.txt`, checks the same grammar, and requires the directory name to match. An older directory still opens when its name and `owner.txt` carry that same UUID.

`filehost/` inside that directory is the only directory passed to `FileHost::open_local`. Upload, download, abort, and read do not take a caller filesystem path. The platform lock file beside `filehost/` belongs to FileHost. This crate does not delete it and does not replace the lock.

A guessed object key, a `sessions/<id>/outputs/` path, or `../` is not a registration. `ObjectKey::parse` rejects those shapes. A key that parses but which `read_manifest` cannot show for the caller's project and principal is not written into the inventory.

## What is registered

Published registration calls `read_manifest` and stores only the manifest the host returns: contract, object key, session, project, principal, grant, digest, size, part count, idempotency key, and lineage. The caller also supplies one role and one policy id. Those two strings are not in the FileHost manifest. They use the same identity grammar as a project id. The grant is stored as hex so the inventory line format cannot smuggle a path. The caller does not supply a manifest document, a version, or a filesystem path.

Open registration calls `begin` and stores the `UploadSession` that call returns. It does not accept a session id from the caller. `part_count` is `unknown` because the host has no public part listing. `lineage` is `absent` because no manifest exists yet.

`version` is always `absent`. The public manifest has no version field. The local backend's empty version is not an immutability token and is not recorded as one.

A second registration of the same host object with the same class, role, policy, and TTL returns the stored row. A different class, role, policy, or TTL is `Conflict` and does not replace the row.

## Clocks and TTL caps

`Now::Production` reads `SystemTime::now`. `Now::Explicit` is the test clock. The CLI reap path has no timestamp argument and always uses production time.

Caps are frozen from the file-lifecycle tables in `docs/11-deployment-and-operations.md` §9, `docs/09-sandbox-and-agent-runtime.md` §8.4, and `specs/open-ontology-v0.1-spec.md` §7.5. A requested TTL of zero or above the cap is refused. The inventory stores the requested TTL, not a value this crate lengthened.

| Class | Cap | Notes |
|---|---|---|
| `uploaded_source` | 180 days | Distinct from session output. Uploaded originals. |
| `session_output` | 30 days | Policy label on a FileHost upload receipt. This crate does not create `sessions/<id>/outputs/`. |
| `workspace_snapshot` | 24 hours | Default off. `explicit_workspace` must be true. Otherwise registration is refused and no 24h row is created. |
| `transcript` | 180 days | |
| `quarantine` | 7 days | |
| `eval` | 365 days | "1 year" is this fixed length, not a calendar year. |

Knowledge packs and audit anchors are not classes. This crate cannot register them for deletion.

The download-permit cap is 300 seconds, from the 5 minute initial TTL in REQ-SRC-05. `enforce_download_ttl` refuses a larger request. A permit is not a bearer URL. Every pull checks the current tuple again. A matching check does not remain valid after the tuple changes.

Days are 86400 seconds. Expiry is `created_unix_ms + ttl_seconds * 1000`, checked for overflow.

## Download permit

The current tuple is project, principal, one role, and one policy id. Project and principal are compared by `FileHost::open_download`, which is the host admission check. Role and policy are compared to the inventory because the host does not store them.

A project or principal mismatch is the host's `Denied`. A role or policy mismatch still performs that host check when the project and principal match, then drops the download without calling `next_chunk`, and refuses the pull. An expired row is refused from the inventory before bytes are returned. `gateway_url_revocation` on every decision is `not_implemented`. `bearer_url` is `none`. This crate does not speak HTTP and does not claim a 5 second URL revocation.

## Reaper

The expired set is the inventory rows whose `expiry_unix_ms <= now`. Rows that are still active are left in place. The reaper does not scan the store and does not delete a key that is not in the inventory.

Before any host delete call it fsyncs a pre-delete audit and a checkpoint. The audit is a bounded line record, at most 64 records, each hashed with SHA-256. Passing the bound returns `Limit` and does not delete.

Observation uses `open_download` and bounded `next_chunk` reads. The running hash and `is_complete` must match the manifest, and the manifest must match the inventory row. A hash or lineage or size or session mismatch is drift. Drift does not call `delete_completed`. A symlink, a corrupt body, or a store error is not absence. `NotFound` from `open_download` or `read_object` is not absence and is not success.

Published rows call `delete_completed` under the host lock. The order, the tombstone, the single quota release, and `observe_completed_absence` are in `FILEHOST-DELETE-CONTRACT.md`. This crate does not unlink store files and does not edit quota files. `FileHost::new` has no deployment lock. This crate does not use it to delete.

- An open session that is still open uses `abort`. That may remove the session row, the quota reservation, the idempotency pointer, and the part objects the host itself tracks. `dest#N` staging is outside that call. There is no public API that proves those open-upload parts are gone. Status is `aborted_parts_unverified`. Success is false.
- An open registration whose object is already a completed manifest uses the live manifest's lineage and the same completed-object delete. The inventory lineage `absent` is not invented into a digest.
- A published row succeeds only when the second `delete_completed` returns `Ok(Some)`, `observe_completed_absence` returns `Ok(true)`, and `completed_quota_release_count` is `1`. Status is `deleted`.

`deletion_success` is true only when every expired row reached that published success. One open abort in the batch keeps it false. `parent_complete` is always false, including when `deletion_success` is true. An idle reap, with nothing expired, has `deletion_success` false and `idle` true.

A checkpoint left at `pre_delete`, `metadata_api_calling`, `metadata_api_done`, or `quota_released` makes the next reap `restarted_from_partial`. The checkpoint file keeps `absence_verified false` and `deletion_success false`. It is a cursor, not a receipt. Restart calls the host again. The tombstone decides the phase. Quota release stays one.

## Explicitly unsupported

| Seam | Why |
|---|---|
| `filehost_part_absence_observation` | Open `abort` still cannot prove part absence. Five-digit parts of a verified completed delete are covered by `observe_completed_absence`. That does not retire this seam. |
| `filehost_dest_staging_reap` | `dest#N` leftovers are outside the delete call. |
| `filehost_object_version` | The receipt has no version. An empty local version is not recorded and is not compared. |
| `filehost_admission_role_and_policy` | The host delete checks project, principal, and grant. Role and policy stay on the inventory row. |
| `gateway_authenticated_url_revocation` | No HTTP transport and no authenticated download URL. |
| `formula_safe_csv_excel` | No CSV or Excel writer in this crate. |
| `sessions_outputs_prefix` | FileHost keys stay `uploads/<session>/<sha256>/original`. |

`required_integration` returns those seam names. They are remaining host and gateway work. They are not a claim that this crate finished the parent card.

## CLI

`artifact-retention reap --root <operator-uuid-dir> [--pause-after pre_delete|metadata_api|quota_released|none]`

The root's final component must match `owner.txt`. There is no clock flag. Exit 0 means idle or `deletion_success`. Exit 2 means a reap ran and deletion is not verified. Exit 75 means the process is waiting at a checkpoint and has not printed success. Exit 1 is a bad root, a corrupt inventory, or a bad argument. The process prints one JSON object. `parent_complete` in that object is false. It does not print a bearer URL. Exit 0 is not parent acceptance.

## Pins

Standalone workspace. The only path dependency is `../../crates/oo-control` at `=0.1.0`. Registry pins are sha2 `=0.10.9` and tokio `=1.53.1` with `macros`, `rt-multi-thread`, `time`, and `sync`. Those versions are already in the root lock. This crate adds no registry crate and does not edit the root lock. The child lock is a checksum subset of the root lock.
