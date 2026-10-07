# artifact-retention-v1

Contract `oo.artifact-retention/v1`. Crate `artifact-retention`.

This crate keeps a durable inventory of objects a real `oo_control::FileHost` has already issued, and it decides expiry against that inventory. It is not a second object store. It does not invent blobs, manifests, or object versions. It does not close SP05-W06, AT-FILE-05, or AT-SEC-03.

The host API is `oo.control.local-file-hosting/v1` as approved by a209 on the fixed commit `cab5e8e219d1924070df7336f5ab5d4ce7cf40dc`. Public calls used here are `open_local`, `begin`, `put_part`, `complete`, `abort`, `read_manifest`, `read_object`, and `open_download`. `ObjectDownload` is used through those calls. There is no public list, no public head of an arbitrary key, and no public delete of a completed object.

## Deployment

`Deployment::create` takes one directory whose final component is `eef831aa-dba4-4af2-a78b-541bc9e0a311`. The directory must be a real directory. A symlink is refused. A path whose name is not that UUID is refused, including a public user path. The process writes an owner file with the UUID and the `HostLimits` it was given, then `fsync`s the file and the directory.

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

Observation uses `open_download` and bounded `next_chunk` reads. The running hash and `is_complete` must match the manifest, and the manifest must match the inventory row. A hash or lineage or size or session mismatch is drift. Drift does not call `abort`. A symlink, a corrupt body, or a store error is not absence. `NotFound` alone is not absence and is not success.

The only host deletion call is `abort`.

- A completed session returns `Closed` and deletes nothing. The object, manifest, session record, idempotency pointer, and quota reservation stay. Status is `unsupported_completed_object_delete`. `deleted` is false. `absence_verified` is false. A later `read_object` is whatever the host returns. This crate does not turn that into a fabricated `NotFound`.
- An open session can return `Ok` from `abort`. That may remove the session row, the quota reservation, the idempotency pointer, and the part objects the host itself tracks. `dest#N` staging is outside that call. There is no public API that proves the part objects are gone. Status is `aborted_parts_unverified`. Success is false. A later `NotFound` on the manifest is expected for a session that never completed, so it is not proof.
- `FileHost::new` has no deployment lock. This crate does not use it to delete, and it does not write quota files, unlink store paths, or follow a symlink to clean it up.

`deletion_success` is true only when every expired row was deleted through a host API and post-observation proved the object, its manifest sidecars, and its multipart parts absent, with no store error treated as absence. That condition is not reachable with the current host API. `parent_complete` is always false. An idle reap, with nothing expired, has `deletion_success` false and `idle` true.

A checkpoint left at `pre_delete`, `metadata_api_calling`, or `metadata_api_done` makes the next reap `restarted_from_partial`. That run still cannot report success until absence is verified. The CLI can wait after `metadata_api_done` so a test can kill the process before the object step. The object step does not unlink bytes.

Quota and idempotent `begin` stay on the host. This crate does not adjust them. A refused or unsupported reap of a completed object leaves the same session on replay and leaves the project reservation in place.

## Explicitly unsupported

| Seam | Why |
|---|---|
| `filehost_completed_object_delete` | No public call deletes a completed object and its manifest under the deployment lock. `abort` returns `Closed`. |
| `filehost_quota_release_on_completed_delete` | Quota for a completed object is released only by a host delete this API does not have. This crate does not edit quota objects. |
| `filehost_part_absence_observation` | No public call lists or stats `staging/<session>/part/<nnnnn>` after `abort`. |
| `filehost_dest_staging_reap` | `dest#N` leftovers are outside `abort`. The host contract leaves them. |
| `filehost_object_version` | The receipt has no version. An empty local version is not recorded and is not compared. |
| `filehost_admission_role_and_policy` | Host admission compares project and principal only. |
| `gateway_authenticated_url_revocation` | No HTTP transport and no authenticated download URL. |
| `formula_safe_csv_excel` | No CSV or Excel writer in this crate. |
| `sessions_outputs_prefix` | FileHost keys stay `uploads/<session>/<sha256>/original`. |

`required_integration` returns those seam names. They are remaining host and gateway work. They are not a claim that this crate finished the parent card.

## CLI

`artifact-retention reap --root <uuid-dir> [--pause-after pre_delete|metadata_api]`

The root's final component must be the owner UUID. There is no clock flag. Exit 0 means the inventory had nothing expired. Exit 2 means a reap ran and deletion is not verified. Exit 75 means the process is waiting at a checkpoint and has not succeeded. Exit 1 is a bad root, a corrupt inventory, or a bad argument. The process prints one JSON object. It does not print a bearer URL.

## Pins

Standalone workspace. The only path dependency is `../../crates/oo-control` at `=0.1.0`. Registry pins are sha2 `=0.10.9` and tokio `=1.53.1` with `macros`, `rt-multi-thread`, `time`, and `sync`. Those versions are already in the root lock. This crate adds no registry crate. a207 remains the only writer of the connector manifest and the root lock. The child lock is a checksum subset of the root lock.
