# HANDOFF — a342 filehost retention complete review

Verdict: **APPROVE_LIMITED** for `filehost_part_absence_observation` and `filehost_dest_staging_reap`. **WITHHOLD** FILE-D03 / E41, SP05-W06, AT-FILE-05, AT-SEC-03, and the five remaining `required_integration` names. `parent_complete` is false. Fixed archive `cd8f754c1b4db22fc962370c4b29bb846b0fccc3`. Product ancestor `def1f7eea53794449d3975b1987d700727cb7d60`. Addendum ancestor `9cdbc625d02fca2fc3fd31b62ce8cb03692a9b81`. Preoverlay and review `docker-run.exit` are 0. Those exits do not accept the parent card, the ledger, or a release.

Owner UUID `eef831aa-dba4-4af2-a78b-541bc9e0a342`. This review owns only `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a342/SP05/filehost-retention-complete-review/**`. Product and repo root were read-only. No new unit was added.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `SHA256SUMS` (every file here except itself; the sums-file hash is in the evidence commit message)
- `source/rust-inventory.txt` and `source/blobs.txt`
- `source/blobs/oo-control/src/file_hosting.rs`
- `source/blobs/artifact-retention/` including `FILEHOST-COMPLETION-ADDENDUM.md`, `FILEHOST-DELETE-CONTRACT.md`, and `Cargo.lock`
- `source/blobs/review_independent.rs` and `source/blobs/lock_from_baseline.rs`
- `linux/preoverlay/console.log`, `docker-run.exit` 0, ends `preoverlay=pass`
- `linux/review-attempt-1/console.log`, `docker-run.exit` 1, rustfmt diff of the overlay only
- `linux/review-rustfmt/console.log` and `note.txt`
- `linux/review/console.log`, `docker-run.exit` 0, ends `review=pass`
- Author packet `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a338/SP05/filehost-retention-complete`
- Historical review `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a336/SP05/filehost-retention-review`

## What this run measured

Linux/arm64, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. One Rust builder at a time, 6 GiB, 1 CPU. No PostgreSQL. Private network `oo-a342-eef831aa-net`. Private volumes `oo-a342-eef831aa-cargo` and `oo-a342-eef831aa-target`.

Preoverlay, before any review overlay: 16 `file_hosting` tests in 36.71s, 6 lib tests in 0.03s, 14 live tests in 0.24s, lock subset `registry_pins=185` `packages=189`, both clippy `-D warnings` 0.

Review overlay: four lifecycle tests in 0.19s and retention clippy 0. The harness plants canonical staging and lets the host reap it. It SIGKILLs its own child at `data_deleting` and at `data_absent`. Restart releases once, the pointer is reused, and a second 64-byte begin is `Quota` while one reservation is held. The open receipt carries the real grant. The first abort is `Ok`. The `NotFound` plus receipt-reused reap still reports the same receipt. A forged grant does not produce absence. A symlink and a manifest directory keep the reservation.

Console characterization, not a failed assertion:

```
receipt_window observe_while_quota_held=true
notfound_replant observe=true quota_still_present=true
```

Observation can be `Ok(true)` while a quota body is still present. The restored-session abort returns `Ok` and removes that one key. A quota file planted after the session is already gone survives the `NotFound` arm.

## Limits to carry

These five names remain in `required_integration()`:

- `filehost_object_version`
- `filehost_admission_role_and_policy`
- `gateway_authenticated_url_revocation`
- `formula_safe_csv_excel`
- `sessions_outputs_prefix`

Plan SP05-W06, AT-FILE-05, and AT-SEC-03 stay open. `bearer_url` is `none`. Gateway URL revocation is `not_implemented`. Session keys stay `uploads/<session>/<sha256>/original`. `FileHost::new` is still `Unsupported`. The original child is not shrunk.

## Cleanup

`cleanup-done.txt` is the command log. Launch scripts already removed the named proof containers. The log removes network `oo-a342-eef831aa-net`, volumes `oo-a342-eef831aa-cargo` and `oo-a342-eef831aa-target`, and `/tmp/oo-a342-eef831aa-dba4-4af2-a78b-541bc9e0a342`. Nothing is pruned. Foreign containers are left in place.

## Next reader

Treat `linux/review` as the focused result and `linux/review-attempt-1` as the preserved rustfmt failure. Do not read exit 0 as parent acceptance. Do not close the ledger from this directory.
