# FINDINGS — a342 filehost retention complete review

Verdict: **APPROVE_LIMITED** for the two repaired seams the preoverlay and the independent lifecycle actually ran. **WITHHOLD** FILE-D03 / E41, SP05-W06, AT-FILE-05, AT-SEC-03, and the five remaining `required_integration` names. `parent_complete` is false on fixed commit `cd8f754c1b4db22fc962370c4b29bb846b0fccc3`. Exit 0 is not acceptance of the parent card.

The reviewed bytes are the a338 product. `file_hosting.rs` blob `8943ac612ca39be64440d8301dd12b48b5d39976`, sha256 `f37377cb0bc359c84c2efb1bce55a69d429770e334bec9544906daca49e6e5ce`, 4804 lines, zero occurrences of `unsafe`. Addendum blob `3a9de3efdca24ec03b3b4e197a1119f2afa8a2cd`. Root lock blob `45d24ef0a15b2db24fc0b9dd02826cd1056ecae8`. Child lock sha256 `41ae41f864e1dc1a6089b3ef70a649949744273fe887056b4fa2cb57828cfc70`. The review overlay is `review_independent.rs`, sha256 `2373a5e20a86fcac2c8d89135982310f4bf6ab512cbb8ced2ff7e72c9f4901b1`, 937 lines, `forbid(unsafe_code)`.

## Passed subparts

`filehost_part_absence_observation`. Preoverlay `open_abort_proves_part_absence_and_reaps_platform_staging` passed. The independent open reap registers an open row, puts one part, and plants canonical `original#1` and `00001#1`. The reaper's first abort path returns status `aborted`, seam `none`, `deleted` true, `absence_verified` true. The receipt body contains the live session, project, principal, and grant. `grant_hex` matches `hex` of the grant bytes and `row.grant_hex`. A wrong grant and a wrong project are `Denied`. A second `abort` is `NotFound`, and a second reap still reports `aborted` with the same receipt bytes.

`filehost_dest_staging_reap`. The host unlinks a regular file whose suffix is canonical: non-empty, no leading zero, all digits. `original#1` and `00001#1` are gone after the open reap and after the completed restart. `original#01`, foreign `dest#1`, `notes`, and a file outside the deployment root remain. The review tests do not unlink those platform names to manufacture absence. The symlink refusal leaves the symlink in place until the test restores its own fixture after the refusal is recorded.

Completed-object quota order. SIGKILL at `data_deleting` leaves bytes and quota, count 0. SIGKILL at `data_absent` has removed the object and still holds quota body `64`, count 0, tombstone `data_absent` / `bytes_then_quota` / `release_count 0`. Each restart releases once. A repeated reap stays at 1. The same upload config rewrites the existing idempotency pointer to the new session id. A second 64-byte reservation is `Quota` while one key is held. `parent_complete` stays false on the JSON that reports `deletion_success` true.

## Receipt window

`cleanup_open` writes `meta/open-absence/<session>` and then deletes the quota key, the pointer, and the session inside one `abort` call. The reaper's open pause is `metadata_api`, which is after `abort` returns. There is no external pause between the receipt write and the metadata delete, so this review did not SIGKILL that window.

The reconstruction copies the host-written quota, session, and pointer, runs a real `abort` that returns `Ok`, and restores those three files. Observation while the quota body is still `64` is `Ok(true)` (`observe_while_quota_held=true`). `open_clear_keys` checks platform staging of the quota key path. It does not require the quota body itself to be absent, which matches addendum lines 107–116 and the `Ok(true)` definition. While that restored body exists, another 64-byte begin is `Quota`, the same config resumes the same session, and exactly one quota file is present. The next `abort` returns `Ok` and removes that key. Only then is `abort` `NotFound`. A wrong grant stays `Denied`. The receipt bytes do not change.

## NotFound arm and a forged receipt

Addendum line 132 says an open row succeeds only when `abort` returns `Ok` and `observe_open_absence` returns `Ok(true)`. `finish_abort` also treats `Err(NotFound)` plus `Ok(true)` as `aborted`. After the session key is already gone, replanting only the quota file leaves `abort` as `NotFound` and the body in place, while observation stays `Ok(true)` (`notfound_replant observe=true quota_still_present=true`). The `NotFound` arm does not delete that replanted key. This review records that as a limit of the extension. It does not withhold the first-abort `Ok` proof, and it does not treat the limit as parent acceptance.

A well-formed receipt with a different grant, planted before abort, makes `write_open_absence` return `Conflict` after parts and canonical staging are already removed. The reap reports `deletion_success` false, `deleted` false, and `absence_verified` false. The quota body stays `64`. Foreign `dest#1` remains. The real grant is `Denied`. The forged grant is `Conflict` because the session record still disagrees. Absence is not claimed.

## Seams that stay

`required_integration()` returns exactly:

- `filehost_object_version`
- `filehost_admission_role_and_policy`
- `gateway_authenticated_url_revocation`
- `formula_safe_csv_excel`
- `sessions_outputs_prefix`

Role and policy stay on the inventory row. The independent permit check returns `role_or_policy_mismatch`, `bearer_url` `none`, and `gateway_url_revocation` `not_implemented`. Session keys stay `uploads/<session>/<sha256>/original`. There is no object version, no download URL, no CSV or Excel writer, and no `sessions/<id>/outputs/` prefix in this proof. `FileHost::new` remains `Unsupported` before store IO; that case is the preoverlay lib test, and the review overlay does not add an `object_store` dependency to reimplement it. Workspace TTL stays 24 hours and default off in the author live test. Uploaded source stays a separate class. `abort` of a completed session stays `Closed` in the author suite.

Plan SP05-W06, AT-FILE-05, and AT-SEC-03 stay open. This directory does not shrink the original child and does not mark a parent checkbox.
