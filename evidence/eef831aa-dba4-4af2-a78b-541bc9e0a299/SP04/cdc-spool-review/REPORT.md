# CDC spool review

Session `eef831aa-dba4-4af2-a78b-541bc9e0a299`. Unit CDC-D02 / SP04-W04. Fixed commit `96afe70fd68a56ae8eec7504bf20e09ecd3cdf65`, tree `672f73fddb93878ae858aa74999061e8d824cc65`.

Limited verdict: on this fixed archive, the durable spool kept the bounded stream, the three SIGKILL windows, and the fault refusals exercised below. Parent acceptance is false. SP04 stays open. This review does not release the crate and does not check the parent goal.

`ack_allowed`, `target_applied`, and `cross_table_consistent` stay false. `applied` stays null. `debezium_normalizer`, `p2_verified`, and `p11_verified` stay false. A fenced segment writer and a real Debezium normalizer are still absent. Callers must keep ACK false.

No product file, root lock, or connector manifest was edited. a207 remains the sole writer of those locks.

## What was reviewed

Author evidence is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a291/SP04/cdc-spool`. That tree was read and left in place. This review's files are only under this directory.

The product is the standalone workspace `tools/cdc-spool`, contract `cdc-spool-v1`, input grammar `cdc-transaction-v1`. Builds used `git archive` of the fixed commit, then `cp -R` inside the container. The live workspace was not the build tree.

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, Rust 1.94.0. Each new builder had 6 GiB, 1 CPU, `no-new-privileges`, CapDrop ALL, private bridge `oo-a299-eef831aa-net`, HOME `/tmp`. PostgreSQL was not started. The pinned Postgres image was not used.

Recipe origins and the small launch changes are in `recipe-origin.txt`.

## Author suite before the overlay

`linux/preoverlay`, container `oo-a299-preoverlay`, id `e01311c2ab9ae288bdb1762f297b617ad69e0e89dbfc00b96d3af9159093d58d`.

Capacity file before start: `own=0 global=1 host_cargo=0`. Own builders were 0. The global count was under 10. Host cargo was 0. Postgres was 0.

Host config: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a299-eef831aa-net`, CapDrop ALL, Memory 6442450944, NanoCpus 1000000000.

Raw exits: docker 0, version 0, fetch 0, clippy-add 0, clippy 0, test 0, registry 0. `author-totals.txt` is `34 0`.

`linux/preoverlay/test.stdout` result lines: library 3, binary unit 0, cli 4, fault 5, protocol 21, scale 1 in 61.98s, doctests 0. Sum 34 passed, 0 failed. The overlay file was absent from that tree (`overlay-absent.exit` 0).

`linux/preoverlay/registry.txt` ends `summary registry_triples=11 missing=0 mismatch=0 pins_present=true path_packages=1`. `lock-before.txt` and `lock-after.txt` are both `d1f2e4745584a100309327f31db45082d63cd66285dae433cc32afa220ccca55`.

`linux/preoverlay/author-seal-hwm.txt` is `34004992`. That is this process's seal-time `VmHWM`, written by the author scale test before its replay loop. It is not required to equal the earlier author reading.

Repo head recorded at this launch: `904e9e45420dede8acfcd9434ebaabeb5a9ae594`. The fixed commit was an ancestor.

## Review overlay attempts

The overlay is Safe Rust in `overlay/a299_boundary.rs`. It has its own SHA-256. It does not call the author test helpers or the product hash as the expected digest. Expected counts, order, values, and digests are hand formulas in that file. `#![forbid(unsafe_code)]`.

| attempt | docker | clippy | test | registry | stop |
|---|---|---|---|---|---|
| linux/overlay-1 | 101 | 101 | absent | absent | `Spool` has no `Debug`, so four `unwrap_err` calls on `create_new` did not compile. `BufRead` was unused. |
| linux/overlay-2 | 101 | 101 | absent | absent | `write_lines` was unused under `-D warnings`. |
| linux/overlay-3 | 0 | 0 | 0 | 0 | 13 tests passed |

Both failed directories stay, including their `a299_boundary.rs` copies and `clippy.stderr`. They are review-harness compile failures. The product archive was compared to the private copy before Clippy, and `source.exit` is 0 on each attempt. No product change followed them.

`linux/overlay-3` container `oo-a299-overlay-3`, id `f1d79d3260074d0b5ae8b2351c6bd42efff536022e0fcb9ed29dab42b8807fca`. Capacity before start: `own=0 global=0 host_cargo=0`. Host config: ExitCode 0, OOMKilled false, Privileged false, same network, CapDrop ALL, Memory 6442450944, NanoCpus 1000000000. Repo head at that launch: `72dc9e2db841de4de368216bbbe13d738f850ea9`.

`linux/overlay-3/test.stdout`: 13 passed, 0 failed, 119.05s, `--test-threads=1`.

- `known_literals_match_the_independent_oracle`
- `five_hundred_thousand_events_match_hand_formulas`
- `ordinal_32767_seals_and_32768_and_65535_are_not_stored`
- `sigkill_before_chunk_sync_is_incomplete_without_ack`
- `sigkill_before_end_is_incomplete_without_ack`
- `sigkill_before_final_seal_is_incomplete_without_ack`
- `torn_seal_is_unknown_without_ack_and_keeps_files`
- `exact_replay_matches_and_payload_conflict_writes_nothing`
- `missing_truncated_and_checksum_chunks_are_refused`
- `manifest_and_seal_checksums_are_refused`
- `foreign_existing_paths_are_not_overwritten`
- `disk_quota_is_an_internal_counter`
- `end_too_early_differs_from_collection_unknown`

Registry summary on overlay-3 matches the preoverlay line: 11 triples, 0 missing, 0 mismatch, pins present, one path package `cdc-spool`. Lock hash unchanged at `d1f2e4745584a100309327f31db45082d63cd66285dae433cc32afa220ccca55`.

## Author failures left in a291

Those directories were not copied here. Raw exits re-read from `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a291/SP04/cdc-spool`:

| attempt | docker | test | clippy |
|---|---|---|---|
| linux/impl-1 | 101 | 101 | absent |
| linux/impl-2 | 101 | 101 | absent |
| linux/impl-3 | 101 | 101 | absent |
| linux/impl-4 | 101 | absent | 101 |
| linux/impl-5 | 101 | absent | 101 |
| linux/impl-6 | 101 | absent | 101 |
| linux/impl-7 | 0 | 0 | 0 |

impl-3 `test.stdout` shows protocol `FAILED. 19 passed; 2 failed`, with `left: CollectionUnknown` and `right: EndTooEarly` on `end_too_early_stays_open_and_a_later_end_can_seal`. The author then changed that expectation. This review's `end_too_early_differs_from_collection_unknown` passed on the fixed archive: an end that names `public.orders` before any row is `CollectionUnknown`; after one admitted row, an end with count 2 is `EndTooEarly`, and a later matching end seals.

`linux/impl-7/scale-hwm.txt` is `27754496`. That is the author process's seal-time reading. See Findings for this run's numbers.

## Memory readings on this review

| reading | bytes | file |
|---|---|---|
| author suite, this preoverlay, seal time | 34004992 | `linux/preoverlay/author-seal-hwm.txt` |
| overlay-3 seal, before the 500000 replay | 30089216 | `linux/overlay-3/seal-hwm.txt` |
| overlay-3 after the iterator and `verify_replay` | 47861760 | `linux/overlay-3/replay-hwm.txt` |
| author impl-7 seal time, earlier process | 27754496 | a291 `linux/impl-7/scale-hwm.txt` |

`replay-hwm.txt` also records `prior_author_seal_not_this_run=27754496`. The overlay asserts the seal reading is positive and below 256 MiB, and that the post-replay reading is at least the seal reading. It does not require equality with 27754496. The seal figure is the `VmHWM` stored on `SealSummary` inside `finish`, before the replay loop. The post-replay figure is a later reading of the same process.

## Inventory

`product.sha256` and `MANIFEST.sha256` are produced after this report, by the byte-copied a199 `tools/sha256_manifest.rs` (`d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`), over the fixed-commit archive at `/tmp/oo-a299-eef831aa-dba4-4af2-a78b-541bc9e0a299/preoverlay/archive/tools/cdc-spool`. The host writes these files after that hash, so they are raw records and are not rows in `MANIFEST.sha256`: `linux/manifest/console.log`, `linux/manifest/docker-run.exit`, `linux/manifest/hostconfig.txt`, `linux/manifest/container-removed.txt`, and `CLEANUP.md`.
