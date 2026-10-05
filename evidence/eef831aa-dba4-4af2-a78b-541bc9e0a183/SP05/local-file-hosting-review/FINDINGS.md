# FINDINGS — a183 fixed local file hosting review

Source lines are from `755fb84acc7f80e5ba2fba1c2b80412dab7fab26`, tree `9bb6da0a68e8533831f3d92fe105caf1423e2c0c`. The oracle snap's six product blobs match `source/blobs.txt` (`oracle/product-files-match.txt`).

## F1 — Limited approval of the local primitive

The fresh oracle on a real `LocalFileSystem` passed the contract checks this review was asked to run.

- `resume_complete_and_lineage` stored part 2 before part 1, refused the short complete with `Incomplete`, kept the object key absent, accepted the same part bytes again, refused different bytes with `Conflict`, dropped the host, resumed with a different grant string, and completed. The manifest contract is `oo.control.local-file-hosting/v1`. Display name `季度 "a".csv` and grant `g "α" \ z` round-tripped. The idempotency digest and the lineage digest computed in `oracle/review_oracle.rs` matched the host. A second `complete` returned the same manifest. `read_object` returned the concatenated bytes. Mutating the object key made `read_object` return `Corrupt`. A file outside the store prefix was unchanged.
- `two_contexts_grant_is_not_authority` used one grant string for the owner, another project, and another principal. Foreign `read_manifest`, `read_object`, `put_part`, `complete`, and `abort` returned `Denied`. `FileHostError` display is the fixed sentence at `file_hosting.rs:72`. The foreign abort left the key set unchanged. The same project and principal with a different grant still read the object, and the stored grant stayed the first one (`file_hosting.rs:400`, `file_hosting.rs:1036`). A wrong digest was `NotFound` for both contexts (`file_hosting.rs:1193`).
- `invalid_complete_hides_the_object` returned `Checksum` for the right length and the wrong bytes, and `Size` after the part was overwritten with a longer body. The object key, the five-digit part, and the session record were gone. `staging/<session>/part/notes` remained. `cleanup_open` deletes numbered parts only (`file_hosting.rs:834`, `file_hosting.rs:1179`).
- `abort_deletes_owned_keys_only` removed exactly `staging/<session>/part/00001`, `meta/session/<session>`, `meta/quota/<project>/<session>`, and `meta/idempotency/<project>/<principal>/<idem>`. The note file and `meta/quota/<project>/notes` remained. A second abort was `NotFound`. Abort after complete was `Closed` and the object bytes remained.
- `concurrent_complete_returns_one_manifest` ran two `complete` calls on one session. `oracle-notes.txt` records `oks=2 errs=`. The follow-up `complete`, the stored bytes, and the independent lineage digest matched. This is one pair on one local store.

`FileHost` debug text omits the store (`file_hosting.rs:344`). The test checked that the scratch marker is absent from that debug text.

## F2 — The quota race is a real over-reservation, so the spec quota stays outside the approval

Contract line 83 says two concurrent `begin` calls can both pass the check before either reservation is visible, and that the race is open.

`reserve_quota` (`file_hosting.rs:623-651`) sums `meta/quota/<project>/<32-hex>` and returns before any write. The reservation is written later by `put_create` (`file_hosting.rs:420-429`).

Fresh run, `HostLimits::try_new(16, 16, 4, 10)`:

- The second sequential 8-byte `begin` returned `Quota`. The passing test asserts that.
- One concurrent pair both returned `Ok`. `oracle-notes.txt`: `quota_one_attempt oks=2 quota_errors=0 reserved=16 limit=10`.

Sixteen reserved bytes against a ten-byte ceiling is the disclosed race, observed once. Sequential refusal is what the primitive does. The plan text at `specs/open-ontology-v0.1-plan.md` SP05-W01, "资源不足停止接收而非无限缓存", and REQ-SRC-05's configurable deployment quota are not satisfied by narrowing "quota" to the sequential path. This review does not approve that guarantee.

## F3 — `complete` materializes the whole part; the 8 KiB stream is a different path

`GetResult::bytes` for a local file reads the whole range into one buffer (`object_store` 0.14.2 `src/lib.rs:1859-1870`, extracted under `control/dependency-meta/object_store-src`). `into_stream` uses `CHUNK_SIZE` 8192 (`src/lib.rs:1895`).

`FileHost::get_bytes` (`file_hosting.rs:880-889`) calls `bytes()` and then `to_vec()`. `part_map` (`file_hosting.rs:752`), `hash_parts` (`file_hosting.rs:772`), and `publish_parts` (`file_hosting.rs:797`) each go through that helper. `hash_object` (`file_hosting.rs:815-831`) is the streaming path, and it runs on the published object.

Fresh measurement, one 8388608-byte part written in 8192-byte `std::fs` chunks so the test did not hold the buffer:

| Sample | Value |
|---|---|
| `memory_store_stream_max_chunk` | 8192 |
| `memory_store_stream_total` | 8388608 |
| `memory_hwm_before_kib` | 4252 |
| `memory_hwm_after_kib` | 20660 |
| `memory_hwm_delta_bytes` | 16801792 |
| `memory_rss_before_kib` | 4252 |
| `memory_rss_after_kib` | 20660 |

16801792 is two times 8388608. That matches one `get_bytes` holding the `Bytes` buffer and the `Vec` together. Later passes can reuse that high-water mark; this sample does not split them apart. RSS after the call was still the new high-water mark. The contract documents the in-memory multipart handle and the `dest#N` file (contract line 65). It does not document a numeric process budget. `docs/17-rust-implementation.md` line 77 still requires streaming large objects. A part may be the whole object, because `HostLimits` allows `max_part_bytes == max_object_bytes`. The parent streaming requirement stays open.

## F4 — Grant evidence is stored input

`GrantEvidence::new` checks length and printable characters (`file_hosting.rs:141-147`). `authorize_record` compares project and principal (`file_hosting.rs:1046-1053`). The idempotency domain omits the grant (`file_hosting.rs:978-987`). The lineage domain includes it (`file_hosting.rs:1008-1027`). The oracle recomputed both digests with `sha2` in the test file and they matched. An unchanged grant string did not authorize another project or another principal. This slice stores the grant. It does not sign it or authenticate it. Contract line 116 already leaves that check open.

## F5 — a181 SHA256SUMS is six source files

`author-sha256sums-gap.txt`: `author_data_lines=6`, `author_file_count=295`, `source_hash_mismatches=0`, `inventory=source_commit_files_only`, `full_evidence_manifest=no`. The six source hashes match commit `755fb84`. The a181 reports, raw logs, and `impl/attempt1` through `impl/attempt6` are outside that file. This review's `SHA256SUMS` covers every file in this directory except the manifest itself.

## F6 — The oracle container passed; the host post-step parser failed

`oracle/docker-run.exit` is 0 and `oracle/phases/worst.exit` is 0. `oracle/launch.sh` then exited 1. `shasum` received a `source/blobs.txt` line as a pathname. `oracle/product-files-after.txt` from that step contained only the header. The follow-up hash of the surviving snap matched all six product files (`product_files_match=1`). The failure is preserved in `oracle/host-post-exit.txt` and in the empty first version being replaced by the recomputed file, with the method line inside that file. The container console was left as the run wrote it.

## F7 — Five new crates, no existing checksum churn

See the table in `REPORT.md`. `control/lock-added.txt` is those five rows. `control/lock-version-added.txt` says `added version alongside existing: itertools 0.15.0`. Checksums of itertools 0.10.5 and 0.14.0 in `source/Cargo.lock` are unchanged by this commit. The `fs` feature is enabled. The `http` feature is not. `http` v1.5.0 is still linked because `object_store` depends on the `http` crate at manifest version `1.2.0` (`control/dependency-meta/object_store-0.14.2.Cargo.toml`). `cargo deny` was not run. `ci/deny.toml` allows Apache-2.0 and MIT and warns on multiple versions; itertools now has three versions in the lock.

## F8 — What this review does not close

S3, MinIO, the HTTP backend, authentication, the reaper, public multi-instance quota, `dest#N` cleanup, and REQ-SRC-05 after `stored` remain open. One concurrent `complete` success and one concurrent quota over-reservation are single samples. The publish-then-delete window at `file_hosting.rs:555-558` was not observed to destroy a finished object in that sample. The contract still leaves a crash between those keys to the reaper.
