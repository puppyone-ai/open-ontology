# Findings

Scope is the frozen CDC-D02 / SP04-W04A spool. No new product unit was opened. Parent acceptance is false.

## Protocol fixes after impl-3

`linux/impl-3` compiled and ran the library, CLI, and fault tests. Two protocol tests failed.

`end_too_early_stays_open_and_a_later_end_can_seal` sent an end that named `public.orders` before any data row. The spool returns `CollectionUnknown` when the declared names differ from the admitted names, which matches `tools/cdc-transaction` `check_counts`. `EndTooEarly` applies only after the names already match and the admitted count is still short. The test now admits one orders row, ends at count 2, then admits the second row and a matching end. impl-7 passed that test, and the directory sealed only on the second end.

`lowered_event_cap_and_active_txn_and_barrier` expected `EventCap` when `global_txn_sequence` exceeded `max_events`. The gate returned `SequenceOutOfRange` first. A global sequence above the cap is now `EventCap`. A collection sequence above the cap stays `SequenceOutOfRange`. Zero stays `SequenceInvalid`. impl-7 passed the lowered cap.

## Clippy

Rust 1.94 Clippy `-D warnings` rejected the library on impl-4 and the tests on impl-5 and impl-6. The repairs that change structure:

- `Recovery::Committed` holds `Box<Committed>` so the enum is not a 576-byte variant.
- Replay and the sample iterator use `for` instead of `while let Some(item) = iter.next()`.
- The checkpoint field uses the alias `CheckpointHook`.
- The independent oracle loads the first 16 words with `zip` and `chunks_exact`.
- Odd collection sequences in the scale fixture use `u64::div_ceil`.

Allows that remain, each on the item Clippy measured as over the default threshold:

- `Manifest`: `struct_excessive_bools`, because the seven attestation flags are contract fields and stay false.
- `apply_data`: `too_many_arguments`, `cognitive_complexity`.
- `apply_end` and `open_committed`: `cognitive_complexity`.
- `verify_replay`: `too_many_lines`, `cognitive_complexity`.
- the sample protocol test, the duplicate-conflict protocol test, and the 500000-event test: `too_many_lines`.
- `CheckpointHook`: `type_complexity`.

impl-7 Clippy exit is 0.

## Durability and faults

SIGKILL is `Child::kill` and the tests require signal 9. impl-7 passed all three windows:

- `BeforeChunkSync` leaves `chunk-000001.partial`, no final chunk name, recovery `Incomplete`.
- `BeforeEnd` leaves synced chunks and `chunks.idx`, no manifest, recovery `Incomplete`.
- `BeforeFinalSeal` leaves `manifest.json` and chunks, no `SEAL`. A torn `SEAL` is `Unknown`.

Owned files from those kills are left in place. Drop does not seal, roll back, or delete. The disk cap is an internal byte counter. `disk_quota_is_a_byte_counter_and_keeps_written_files` sizes the cap to the first event plus the index and progress allowances, refuses the second event with `DiskQuota`, and still writes a probe file in the same directory. That is the deterministic cap. It is not an OS quota.

`linux/impl-7/scale-hwm.txt` is `27754496` bytes, read from `/proc/self/status` `VmHWM` inside `finish` at seal. The assertion is on that reading. The figure is not a peak for the later 500000-row replay loop.

## Limits that stay in force

- `ack_allowed`, `target_applied`, `cross_table_consistent`, and the other attestations are false. `applied` is null. Durable progress is not applied progress.
- Replay of a sealed directory must match event hashes. A payload conflict returns `PayloadConflict` and writes nothing.
- An existing path, a seal, a foreign pending directory, or a parent symlink is refused. `create_new` does not replace it.
- A crash during sync or before `SEAL` is `Incomplete` or `Unknown`. The spool does not report rollback or deletion for that state.
- `SequenceGap` is still produced by `check_bits` when a bitset is internally inconsistent. The public tests reach `SequenceOutOfRange`, `EndTooEarly`, and `CollectionCountGap` for ordinary holes.

## Out of scope

A fenced target writer, durable quarantine, a control offset, spool TTL, a real Debezium normalizer, source acknowledgement, and any database or Valkey write remain parent work. This crate does not mark SP04 complete and does not publish or install.
