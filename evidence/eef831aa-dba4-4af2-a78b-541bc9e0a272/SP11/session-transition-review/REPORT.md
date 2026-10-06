# Session transition independent review

Verdict: **APPROVE_LIMITED** for the pure reducer and CLI in `tools/session-transition` at `b0f7da4771e3c3625a45329c721e174f4380571c`, built from the fixed archive `9951cda7f128e30f1f6ce02e409d542d20a9e4fb`.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a272`. This directory is the independent review record. The author session `eef831aa-dba4-4af2-a78b-541bc9e0a265` and its `linux/review-1` are the same author's run. They are not this review. No product file, root lock, spec, or ledger was edited. This file does not accept parent RT-D01, does not close SP11, and does not certify a physical release.

The crate is a pure function. `transition` reads the caller-supplied state, identity tuple, expected revision, expected owner generation, event id, `now_ms`, and event. `database_used`, `auth_provider_used`, `runtime_used`, and `physical_release_certified` stay false on the outputs exercised here.

## Decisive runs

`linux/proof` is one container, `oo-a272-proof1`, id `36a95d97302e8eb99b08796de4c941fb1af2a0a9cc811876b86e280730d7b8b2`. HostConfig: ExitCode 2, OOMKilled false, Privileged false, NetworkMode `oo-a272-eef831aa-net`, CapDrop `["ALL"]`, Memory `6442450944`, NanoCpus `1000000000`. `capacity.txt` recorded `own=0 global=1 host_cargo=0 cap_global=10 cap_own=1 postgres=0`. Rust `1.94.0` `(4a4ef493e 2026-03-02)` `aarch64-unknown-linux-gnu`. Cargo `1.94.0`. Debian 13. `repo-head.txt` is `eb9fdf46313d8a1d3cc4e25eb1fa09be28e3842e`.

`docker-run.exit` is 2. The failing phase is the first registry parser. `lock-subset.exit` is 1, `lock-subset.stderr` is `lock_subset: oo-analysis 0.1.0 has no registry source`, `lock-compare.exit` is 2, and `lock-summary.exit` is 2. That parser treated every source-less package as an error, so the root lock's workspace members failed the parse. The product phases in the same container then ran and exited 0. The corrected parser was run later in `linux/lockcheck`. The proof console and exit files were left as they ran.

Product phases inside `linux/proof`:

- `version.exit` 0, `blob-cmp.exit` 0, `overlay-absent.exit` 0, `lock.exit` `present`, `lock-compile.exit` 0
- `fetch-offline.exit` 101 on the fresh private cargo volume (`no matching package named serde_json` under `--offline`)
- `fetch.exit` 0
- `smoke-test.exit` 0 before the overlay was copied, `smoke-count.txt` 15, `smoke-names.exit` 0
- `overlay-present.exit` 0, `test.exit` 0
- `clippy-component.exit` 0, `clippy.exit` 0, `clippy.stdout` empty, Clippy stderr ends `Finished dev profile [unoptimized + debuginfo] target(s) in 1.97s`
- `product-unchanged.exit` 0, and `src-before.txt` matches `src-after.txt`

`linux/lockcheck` is a second container, `oo-a272-lockcheck`, id `1abf212136a4115c291ba5736b0f22109cfab82d179135e2accc2b8a51023631`. NetworkMode `none`. Same memory, CPU, and CapDrop. ExitCode 0, OOMKilled false, Privileged false. `capacity.txt` again recorded `own=0 global=1 host_cargo=0`. It compiled `tools/lock_subset.rs` with `rustc --edition 2024 -D warnings` and did not run Cargo against the crate. `version.exit` 0, `lock-compile.exit` 0, `lock-subset.exit` 0, `lock-compare.exit` 0, `lock-summary.exit` 0, `docker-run.exit` 0.

## Author smoke, before the overlay

`cargo test --locked --offline -- --test-threads=1` on the sealed archive, with the review overlay absent. `smoke-test.stdout`:

- lib `sha256::tests::known_answers`: 1 passed
- bin: 0 tests
- `cli_roundtrip`: 2 passed (`cli_rejects_trailing_json_and_arguments`, `cli_roundtrip_reports_the_post_state_revision`)
- `handcomputed`: 12 passed
- doctests: 0

The passed fields sum to 15. Compile line in `smoke-test.stderr`: `Finished test profile [unoptimized + debuginfo] target(s) in 5.98s`. The handcomputed batch finished in 1.72s. Names: `cancel_then_begin_turn_is_stopped`, `changed_actor_beats_stale_and_keeps_workspace`, `full_journal_does_not_drop_and_still_replays`, `happy_path_revisions_are_hand_counted`, `identical_replay_is_idempotent_and_conflict_refuses`, `overflow_does_not_wrap`, `reason_names_and_bounds`, `releasing_stops_admission_and_close_needs_s8_absence`, `stale_revision_and_generation_leave_the_snapshot`, `takeover_heartbeat_and_turn_timeout`, `time_boundaries_refuse_at_the_exact_instant`, `tool_sixty_is_the_last_admitted_start`.

The author's full-journal test fills the journal with heartbeats and replays the first id. The release-on-a-full-journal case is in the overlay.

## Independent overlay

`cargo test --test a272_handcomputed --locked --offline -- --test-threads=1`. `test.stdout`: `ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s`.

Names: `cli_malformed_and_invalid_state`, `dedup_replay_conflict_and_overflow`, `full_journal_refuses_release`, `identity_is_not_authentication`, `malformed_snapshots_refuse`, `product_sha::tests::known_answers`, `release_is_caller_declared_and_closed_is_immutable`, `sha256_known_vectors`, `supplied_revision_and_generation_are_the_cas`, `time_boundaries_and_lease_instant`, `tool_sixty_then_refusal`.

Hand counts that passed:

- `begin_bind` from revision 0 generation 0 reaches revision 1, generation 1, phase binding. A second call with expected revision 0 stays revision 1 with reason `stale`.
- `complete_bind` reaches revision 2, generation 1, phase ready, workspace `ws-1`, `last_activity_at_ms` 2000.
- Takeover at the lease instant reaches revision 3, generation 2, phase releasing, failure `owner_lost`, `last_activity_at_ms` still 2000, `same_workspace_allowed` false. A stale generation stays generation 2. A second takeover reaches revision 4, generation 3, failure still `owner_lost`.
- Begin turn at `1_799_999` reaches revision 5. The same instant `1_800_000` is `idle_boundary`. Absolute refusal at `28_800_000` is judged before the idle boundary. Begin turn at `28_799_999` is admitted.
- Tool start at `599_999` reaches revision 7 with `tool_starts` 1. The instant `600_000` is `turn_boundary`.
- Heartbeat at one millisecond before lease expiry is admitted and leaves `last_activity_at_ms` unchanged. Takeover then is `lease_held`. Heartbeat at the expiry instant is `lease_expired`. Takeover at that instant moves generation 3 to 4 and revision 6 to 7, failure `owner_lost`.
- Replay of `e-bind` with `now_ms` 999 against `last_now_ms` 1000 is `time_backward`. Equal `now_ms` is admitted.
- Sixty tool starts from running revision 8 end at revision 68, `tool_starts` 60, ids `t01` through `t60`. The next id `t61` is `tool_boundary` and the count stays 60.
- Identical replay is `idempotent` and ignores the pre-image expected revision. The same event id with a different owner is `event_id_conflict`. A full journal still replays the stored id. A new heartbeat on that journal is `dedup_exhausted`.
- Takeover at generation `u64::MAX` with an empty journal is `generation_overflow`. The same takeover with 1024 journal entries is `dedup_exhausted`. Heartbeat at revision `u64::MAX` with the wrong owner is `owner_mismatch`. The matching owner is `revision_overflow`. The new lease in that heartbeat is `30_001` when `now_ms` is 1.
- Cancel from ready revision 3 reaches revision 4, phase releasing, failure `user_cancel`. Begin turn is `admission_stopped`. Complete release with S8 `actual_absence` false is `s8_not_absent`. A release record that sets S1 while claiming an absence-only shape is `absence_only_s8`. A complete S1–S11 record, S5 skipped, S8 absence true, reaches revision 5, phase closed, `physical_release_certified` false. Later mutations are `closed`. Replay of `e-close` is `idempotent` at revision 5.
- A releasing snapshot whose journal is already `j0000` through `j1023` refuses S8 absence false as `s8_not_absent`. The valid complete release on that snapshot is `dedup_exhausted`. Phase stays releasing, revision stays 9, S1 and S8 stay empty, and the journal endpoints stay `j0000` and `j1023`.
- Malformed snapshots refuse as `state_invalid` and leave the supplied state: 1025 journal entries, a duplicate event id, a non-hex fingerprint, an owner on a requested state, unsorted tool ids, an open tool id that was not started, a running state with no turn, and a closed phase whose release record is incomplete or whose S8 absence is false.
- CLI trailing JSON exits 1 with stderr `session-transition: malformed json` and no revision field. `--help` exits 1 with `malformed usage`. Stdin of `1_048_577` bytes exits 1 with `malformed stdin_too_large`. A requested state that already names an owner exits 2 with stderr `session-transition: refused state_invalid` and stdout revision 0. The four flags on those stdout objects are false.

A changed role with a wrong expected revision is `new_session_required`, keeps workspace `ws-1`, and is not `stale`. A begin bind whose principal is `not-a-directory-user` and whose `claims_hash` is 64 zeroes is applied. `auth_provider_used` stays false. An identity object with an extra `signature` field fails `deny_unknown_fields`.

## Registry triples

`linux/lockcheck/lock-subset.stdout` lists 11 registry triples and `summary registry_triples=11 missing=0 mismatch=0 direct_pins_present=true`. `lock-compare.exit` 0 against `oracle/registry-triples.txt`. The crate lock's registry packages are a subset of the root lock in the fixed archive, checksums included. Direct pins serde 1.0.229, serde_core 1.0.229, serde_derive 1.0.229, and serde_json 1.0.151 are in that list. The other seven are itoa 1.0.18, memchr 2.8.3, proc-macro2 1.0.107, quote 1.0.47, syn 3.0.6, unicode-ident 1.0.26, and zmij 1.0.23. Source for each is `registry+https://github.com/rust-lang/crates.io-index`. The local package `session-transition` 0.1.0 has no registry checksum. Root workspace path packages are skipped by the corrected parser. The proof container's first parser did not skip them.

Crate `Cargo.lock` SHA-256 `9cb340c0cbbc4b0cb561c05796e72702d0714eee16b7515f0e2f3600d58c6adc`, recorded in `linux/proof/source-sha256.txt`. The proof compared that lock to the sealed blob and did not copy it back onto the product tree.

## SHA-256

`sha256_known_vectors` calls the product `sha256` through `#[path = "../src/sha256.rs"]`. The vectors that passed are empty `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`, `abc` `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`, the 56-byte NIST message `248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1`, the 112-byte NIST message `cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1`, one million bytes of `a` `cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0`, and the begin-bind fingerprint preimage `a0bab5c228e31c54a6bb0b10cb29666a891fc089ef098fe344b3640866f16a27`. The author's lib test covers empty and `abc` only. The image contains Python (`python-present.flag` is 1). Python was not invoked.

## Archive

Reviewed commit `b0f7da4771e3c3625a45329c721e174f4380571c`, tree `c746c6413ebf15a44498bb25ba6522f5cbbbafc9`. Fixed archive `9951cda7f128e30f1f6ce02e409d542d20a9e4fb`, tree `7e83a381c8a90c5f61f3ce73947eb7ac441381e9`. The proof archive is `git archive` of that fixed commit for `Cargo.lock`, `rust-toolchain.toml`, and `tools/session-transition`. Source blobs are `git show` of the reviewed commit. `linux/proof/source-sha256.txt` lists the ten blob hashes.

## Budget

Review builders: `linux/proof`, then `linux/lockcheck`, then the manifest pair in `linux/manifest/launch.sh`. One builder at a time, 6 GiB, 1 CPU, 0 PostgreSQL. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Private cargo volume, target volume, and network for the proof. Lockcheck and the manifest use network `none` and no cargo volume. `CLEANUP.txt` records removal of `oo-a272-eef831aa-net`, `oo-a272-eef831aa-cargo`, `oo-a272-eef831aa-target`, and `/tmp/oo-a272-eef831aa-dba4-4af2-a78b-541bc9e0a272`. The pinned image stays. `docker prune` was not run.

## Limits of this approval

The CAS compare uses the revision and generation in the supplied state. `database_used` stays false. This crate has no durable database.

The identity tuple is caller-supplied and syntax-checked. `auth_provider_used` stays false. A 64-hex `claims_hash` is not an authentication.

S8 `actual_absence` is the caller's flag. `physical_release_certified` stays false on the closed state. Physical absence is not certified here.

A journal of 1024 entries still replays a stored id. A new event that passes the earlier checks, including a well-formed complete release, is `dedup_exhausted` and is not journaled. An S8 absence false on that full journal is `s8_not_absent`, because the event rule is judged before the cap.

A closed state admits an identical journal replay. Every other event in the overlay is `closed`.

Session-end S1–S11 timings were not measured. This review had no runtime. Kernel, PostgreSQL, and the parent ledger stay outside this record.
