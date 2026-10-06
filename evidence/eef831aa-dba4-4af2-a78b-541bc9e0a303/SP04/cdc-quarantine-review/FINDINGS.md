# Findings

## Preserved author deviation

Author session a294 wrote `CONTRACT.md` after `model.rs` and `store.rs` already carried the same field list. Git history under `tools/cdc-quarantine` at the fixed commit is one commit, `b650e2ac42f0e783f26db4d6799552cb27f11d0b`, which adds the contract and the source together. There is no earlier contract-only commit. This review left that history as it is.

## Review-test compile failures

These are test-harness failures. The product crate was not patched.

`linux/overlay` exit 101. `tests/review_proof.rs` named `cdc_quarantine::ControlEvent`. `src/lib.rs` does not re-export it. Error `E0425`. The saved source is `linux/overlay/review_proof.failed.rs`, sha256 `e7f290311757483cb22e71cd067cceb411b00af8da1baabd4d26dbb672857cd1`.

`linux/overlay-2` exit 101. The replacement macro borrowed a temporary `control_line(...)` across statements. Error `E0716`. The saved source is `linux/overlay-2/review_proof.failed.rs`, sha256 `b4058bbad866829019da834effed3d06298bfe5096110376466d33a1068457e5`.

`linux/overlay-3` test exit 0, eight tests passed. Clippy exit 101 on `assert_eq!(stored.local_prerequisite, true)` (`clippy::bool_assert_comparison`, denied by `-D warnings`). The saved source is `linux/overlay-3/review_proof.failed.rs`, sha256 `1e430904a4b311b28c4735d606b131deb3d4232e7bc52998222ab78dc3611f9c`. The author suite allows that lint in `tests/proof.rs`. The review test uses `assert!(stored.local_prerequisite)` instead.

`linux/overlay-4` is the run whose test exit and clippy exit are both 0.

## Public types an integration test can name

`parse_control`, `put`, `replay`, `recover`, and `identity_preimage` are public. `ControlEvent` and `Rejected` stay in the private `model` module. An external test can pass the parsed value into those functions. It cannot write the type names. The passing review test therefore checks position and reason on the public `Status` value, and checks extra `ordinal` and `lsn` keys with `parse_control(...).is_err()` plus the CLI fault `malformed`. `Status.fault` for that CLI run is `malformed`, `paused` is false, and `attempt_collection` is null. The review does not inspect `Rejected.code` or `Rejected.event`.

Library clippy `-D warnings` passed in preoverlay, so this shape is what the crate currently ships.

## Durability observed

Seal order in `src/store.rs` is: create the event directory, write `payload.partial`, checkpoint `before_data_fsync`, fsync and reread, checkpoint `before_manifest`, write and fsync `manifest.json.partial` and, for the first event, `STORE.partial`, checkpoint `before_rename`, rename onto absent finals, checkpoint `before_parent_fsync`, fsync the directories, then write `RECEIPT`. Recovery does not seal, delete, or rewrite.

The SIGKILL test is name-level. `CONTRACT.md` says the kernel may flush pages after the signal. The review shows which names exist, that `RECEIPT` is absent, that the child status is signal 9, and that a later recover, replay, and put do not change the inventory. It does not show torn bytes inside a file.

`unknown_incomplete` is true when neither `manifest.json` nor `manifest.json.partial` can be read. `before_rename` and `before_parent_fsync` still parse a collection, so `unknown_incomplete` is false while `incomplete` and `paused` stay true and ACK flags stay false.

## Flags, position, quota, and locks

Manifest bytes keep every attestation false or null, including `local_prerequisite`. Status turns `local_prerequisite` on only for a durable complete record with no fault. The crate does not apply a target schema, issue a target TRUNCATE, or set an ACK flag. No fenced control integration is present in this crate.

`source_position` is stored as the caller decimal. The review value `18446744168199581248` does not fit in `u64`. Provenance on the status is `caller_declared`. `ordinal` and `lsn` are not fields of this contract. An object that adds either key is malformed.

The disk cap is the byte counter described in `CONTRACT.md`. A lowered cap of 0 refuses with `disk_quota` before `create_dir`. The same process can still write a small file through `std::fs`. The event cap is `max_events`, separate from that counter.

The 11 child registry packages are the name, version, and checksum subset listed in `linux/overlay-4/lock-subset.txt`. Root `Cargo.lock` also contains `syn` 2.0.119. The child lock uses `syn` 3.0.6. No lock file was edited.

## Left outside this review

The parent ledger row remains unchecked. This review does not accept CDC-D03 for a parent count and does not claim a release. No connector manifest was edited.
