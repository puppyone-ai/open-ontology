# Findings

Session `eef831aa-dba4-4af2-a78b-541bc9e0a299`. Scope is the frozen CDC-D02 spool at `96afe70fd68a56ae8eec7504bf20e09ecd3cdf65`. No new product unit was opened. No product file was edited.

Limited verdict: the durable spool, on the fixed archive, passed the author suite and the independent boundary tests listed in `REPORT.md`. Parent acceptance is false. SP04 stays open. This review does not release the crate.

## Boundaries that held on linux/overlay-3

The green test log is `linux/overlay-3/test.stdout` (13 passed, 0 failed, 119.05s). Expected values come from `overlay/a299_boundary.rs`, not from the author helpers and not from the product hash function.

500000 events, transaction `rev500`, two collections. Odd sequences are `public.orders` with collection sequence `div_ceil(seq, 2)`. Even sequences are `public.lines` with collection sequence `seq / 2`. The test required 250000 events in each collection, sequence order, key and payload ordinal 0 equal to the sequence, and chunk names `chunk-000001` upward under the product caps (8192 events, 4194304 bytes). File count stayed under 96. Commit LSN `16/B6E40` was checked as `(0x16 << 32) + 0xB6E40 = 94490029632`, and epoch 1 position as `(1 << 64) + that = 18446744168199581248`.

Known digests, recomputed by the overlay SHA-256: empty `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`, `abc` `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`, sample preimage `4864cde4bd37438f0fa3c69808c0adb5ddd244d2450987ad69299330b2b8e8ee`. The sealed manifest bound source `billing`, the repeated `0123456789abcdef` lineage and schema hashes, epoch 1, selected tables `public.lines` then `public.orders`, and provenance `caller_declared`.

Ordinal 32767 sealed and replayed. Ordinals 32768 and 65535 returned `OrdinalRange` and stored no chunk, manifest, or `SEAL`.

Own child SIGKILL used the product binary `cdc-spool spool` with `CDC_SPOOL_CHECKPOINT` and `CDC_SPOOL_MARK`. `Child::kill` was required to be signal 9. The feed is 8192 `public.orders` events because the CLI uses `Limits::product()` and a chunk seals on that cap before end.

- `BeforeChunkSync`: `chunk-000001.partial` nonempty, no final chunk, no manifest, recovery `Incomplete`, `durable_events` 0, no `SEAL`, no `ROLLED_BACK`.
- `BeforeEnd`: `chunk-000001` and `chunks.idx`, no manifest, recovery `Incomplete`, `durable_events` 8192, no ACK.
- `BeforeFinalSeal`: `manifest.json` contains `"ack_allowed":false`, `"debezium_normalizer":false`, and `"applied":null`, no `SEAL`, recovery `Incomplete`.
- A 4-byte `SEAL` body `nope` is `Unknown` with reason `seal_torn`. Chunk and manifest bytes other than `SEAL` stayed. A 64-zero seal of the right length is `ChecksumFault`.

Exact replay of the sample feed matched, including the sample hash literal. Changing the payload byte `a` to `b` returned `PayloadConflict` and left the directory digest tree unchanged. A removed chunk is `MissingChunk`. A short chunk is `Truncated`. A same-length byte flip is `ChecksumFault`. A manifest byte flip with the old `SEAL` is `ChecksumFault`. Owned files stayed.

An existing file, a foreign directory, and an already sealed directory returned `PathExists` and kept their bytes. A parent symlink returned `SymlinkRefused` and did not create the target.

Disk quota is the internal counter in `charge`. The test set `max_chunk_events` to 1 and `max_disk_bytes` to the first canonical line plus `INDEX_ALLOWANCE` 128 plus `PROGRESS_ALLOWANCE` 512. The second event was `DiskQuota`. `chunk-000001` remained. A following write of `still-writable` succeeded in that directory, so the refusal is the counter.

`EndTooEarly` and `CollectionUnknown` stayed distinct, as in the author correction after impl-3. See `REPORT.md` for the impl-3 raw failure line.

Attestations observed on the sealed 500000-event manifest and on the kill manifest stayed false or null: `ack_allowed`, `target_applied`, `cross_table_consistent`, `debezium_normalizer`, `p2_verified`, `p11_verified`, `upstream_commit_lsn_verified`, and `applied`.

## Registry

All 11 registry triples in `tools/cdc-spool/Cargo.lock` are a subset of the root lock from the fixed commit. The checker is `tools/lock_subset.rs`. Both runs printed `registry_triples=11 missing=0 mismatch=0 pins_present=true path_packages=1`. The only path package is `cdc-spool`. The four serde checksums in the author report are among those 11. `cargo update` and `cargo generate-lockfile` were not run. The lock hash did not change during preoverlay or overlay-3.

## VmHWM

`resident_hwm_bytes` is read from `/proc/self/status` inside `finish`, when the seal summary is built, before the caller iterates or replays. The author test writes that summary field before its replay loop. `27754496` is that earlier process. This review recorded `34004992` on the author suite and `30089216` at overlay seal. After the overlay iterator and `verify_replay`, the same process read `47861760`. The later reading is higher. None of these figures is an OS quota, and none is a required cross-run constant.

## Review harness failures that stay

`linux/overlay-1` and `linux/overlay-2` failed Clippy on the review test, before any boundary test ran. The causes are in `REPORT.md` and in each `clippy.stderr`. They are not product defects and they were not fixed by editing `tools/cdc-spool`.

## Author history that stays in a291

impl-1 through impl-6 remain failed evidence in the author directory, with the exits in `REPORT.md`. impl-7 remains the author green run. This review did not delete or rewrite those files.

The impl-3 `SequenceOutOfRange` versus `EventCap` test change is author history already present in the fixed tree. This review re-ran the author protocol file (21 passed, 0 failed) and did not add a second case for that gate.

## Still absent

A fenced segment writer, durable quarantine, a control offset whose ACK is separate from durable counts, spool TTL, a real Debezium normalizer, source acknowledgement, and any database write are parent work. This crate's seal does not mean the target applied the transaction. Callers must keep ACK false until that later writer and a real normalizer exist.
