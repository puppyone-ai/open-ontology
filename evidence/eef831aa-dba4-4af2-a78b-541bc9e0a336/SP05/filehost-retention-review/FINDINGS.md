# FINDINGS — a336 filehost retention review

Verdict: **APPROVE_LIMITED** for the completed-object delete on fixed commit `53931278e69d723d0ce38961ded897fd6e41d5d3`. `parent_complete` is false. SP05-W06, AT-FILE-05, and AT-SEC-03 stay open. This file does not close the ledger and does not add a countable unit.

The limit is the `quota_released` window. Measured on `HostLimits::try_new(64, 64, 10, 64)`:

`quota_window old_bytes=64 reserved=64 max=64 reservation_within_max=true bytes_plus_reservation_exceeds_max=true`

## Which quota number is enforced

`contracts/control/local-file-hosting-v1.md` at the fixed commit says project quota is the sum of `meta/quota/<project>/<session>` bodies whose names are 32 lowercase hex characters, and that `begin` refuses with `Quota` when the new expected size would pass `max_project_bytes`. It also says a completed object keeps its reservation, and that abort and the checksum/size cleanup release it.

`tools/artifact-retention/FILEHOST-DELETE-CONTRACT.md` says it does not replace that approved text. Its delete order is tombstone `intent` with `release_count 0`, delete exactly one quota key, tombstone `quota_released` with `release_count 1`, then optional `Ok(None)` while the object, manifest, session, and idempotency pointer are still present, then delete the five-digit parts, object, manifest, session, and pointer. It says project quota stays the sum of those quota bodies, and that release is removal of the one key named in the tombstone.

The code matches that order. `reserve_quota` sums quota keys. It does not add the length of an object that no longer has a quota key. During `quota_released` the completed object's reservation is already gone and its bytes are still on disk. A resumed `begin` of the same config returns the same session and does not reserve again. A different display name is a new idempotency key. That begin is admitted. The new body is 64. The sum of reservation bodies is 64, so it is within `max_project_bytes`. The object file is still 64 bytes, so disk bytes plus the new reservation are 128. A third begin of 64 is `Quota`.

After the finishing delete, observation is `Ok(true)`, the count stays 1, and the concurrent reservation is still 64. The original config is `Quota` until that new session is aborted. Abort removes the new key and does not move the old count. A later begin of the original config allocates a new session.

The deployment lock is held for one `delete_completed` call and dropped when that call returns. The reaper's pause sits between `stop_after_quota_release=true` and the finishing call, so another `begin` can run while the old bytes remain. The single-task happy-path reap does not itself interleave a begin. The window test calls the stop flag directly.

The sentence "a completed object keeps its reservation" is false for that interval. The addendum requires the interval. The enforced hard check that held in this run is the reservation sum. Disk occupancy is not a second counter. This review does not withhold for that specified window. It would withhold if the reservation sum itself exceeded `max_project_bytes`, or if absence, the single release, or the kill restart failed. Those did not fail.

## Symlink after the release

The same order has a fail-closed stuck state. After `quota_released` and a concurrent begin, the review renamed the object aside and replaced the object path with a symlink. `delete_completed(..., false)` returned `Corrupt`. The symlink stayed. The aside bytes stayed. The new quota body stayed `64`. `completed_quota_release_count` stayed 1. `observe_completed_absence` was not `Ok(true)`. Another begin of the same size was `Quota`.

The host does not unlink the symlink, does not release a second time, and does not put the reservation back on the stuck object. `Corrupt` is not absence. A concurrent upload can hold the whole project quota while those bytes remain. That is recorded with the limit. It is the contract's symlink result, and it is not a passing absence.

## What the public calls did

`delete_completed` returns `Result<Option<ObjectManifest>, FileHostError>`. `Ok(None)` is the stop after quota release. `Ok(Some)` is the verified delete. `observe_completed_absence` returns `Ok(true)` only as the absence proof. `completed_quota_release_count` is 0 or 1. `FileHost::new` still returns `Unsupported` before store IO for the three calls. `abort` of a completed session is `Closed` and leaves the object.

A published object past expiry is deleted through the reaper's calls into FileHost. The review did not unlink the object to fake deletion and did not write a quota body. After the reap, observation is `Ok(true)`, `read_object` is `NotFound`, the tombstone is `phase verified` / `part_count 2` / `release_count 1`, and `parent_complete` is false. A second reap stays at count 1. A foreign uploaded-source object inside its 180 day class stays readable, and its quota body is unchanged.

`part_keys` lists `staging/<session>/part` and keeps names that `part_number_from_key` parses: five digits, number in `1..=MAX_PART_COUNT`. It does not stop at `manifest.part_count`. `dest#1` and `notes` do not parse, so they are skipped. The lifecycle test planted `00001` after `complete` had already removed the published parts. Reap removed `00001` and left `dest#1`, `notes`, `operator-note.txt`, and a neighbor file outside the host directory. The tombstone still records `part_count 2`.

`key_present` requires `local_node` and the store length to agree. A symlink or a non-file is `Corrupt` from `local_node`. A local/store mismatch is `Corrupt`. An IO error is `Store`. `NotFound` on the local node is absence of that key, which is not by itself the public absence proof. `read_object` `NotFound` is not that proof. The manifest-replaced-by-a-directory reap stays `deletion_success` false, `deleted` false, `absence_verified` false, status not `deleted`, object bytes unchanged, and observation not `Ok(true)`.

Wrong project and wrong grant are `Denied` and leave the object. A wrong digest is `NotFound` and leaves the object. Count stays 0. A flipped object byte makes the reap `refused_tamper_or_symlink` and leaves the flipped bytes. A symlink object gets the same status and is not unlinked.

## Kill and pulls

The review spawned `artifact-retention reap --root <uuid-dir> --pause-after quota_released`. There is no `--now`. The clock is `Now::Production` in the CLI. The test registered a one-second TTL created at 0 so the production clock was already expired. After the checkpoint contained `phase quota_released` and `control/waiting` was a file, `try_wait` was `None`. `Command::kill` produced signal 9. Captured stdout did not contain `"deletion_success":true`. The object was still readable and the count was 1. Restart exited 0 with deletion success, `restarted_from_partial` true, `parent_complete` false, and `absence_verified` true. The checkpoint file remained a cursor with both flags false. A second reap left the count at 1.

A download that had already reached `is_complete` stays complete after the delete, and a later `next_chunk` is `Ok(None)`. A pull with chunk budget 32 returned one 32-byte chunk and was not complete. After the delete, its next chunk was `Store` or `Corrupt`, and `is_complete` stayed false. The extra `assert_ne!(observe.ok(), Some(false))` in that test also passes on `Err`, so it is not an absence proof. Absence for the finished delete is the lifecycle test's `Ok(true)`.

## Still open

Open abort stays `aborted_parts_unverified`. `deletion_success` is false on that batch. `dest#1` remains. `filehost_part_absence_observation` and `filehost_dest_staging_reap` stay required.

The plan's SP05-W06 text still requires an authenticated download proxy, URL revocation within 5 seconds, and formula-safe CSV/Excel. The permit for role `viewer` against policy `policy_a` is refused, with `bearer_url` `none` and `gateway_url_revocation` `not_implemented`. Kernel and gateway URL revocation and external auth stay parent requirements.

`required_integration()` remains:

- `filehost_part_absence_observation`
- `filehost_dest_staging_reap`
- `filehost_object_version`
- `filehost_admission_role_and_policy`
- `gateway_authenticated_url_revocation`
- `formula_safe_csv_excel`
- `sessions_outputs_prefix`

`parent_complete` is false on the idle reap, the successful reap, the repeated reap, the kill restart, the tamper refusal, and the open-abort reap.
