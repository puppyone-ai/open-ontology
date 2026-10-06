# Report

Independent review of `tools/cdc-quarantine` at fixed commit `25dfc9022780c3deff12f37b7ad7ea4ed1fb6480` (tree `5903f9c8f39b9cb426df9c5f88b22af316c7826b`). Session `eef831aa-dba4-4af2-a78b-541bc9e0a303`. The product commit that added the crate is `b650e2ac42f0e783f26db4d6799552cb27f11d0b`. `git diff 25dfc902 -- tools/cdc-quarantine` was empty at review time. This review wrote only `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a303/SP04/cdc-quarantine-review/**`.

`AGENTS.md` is absent. The plan row in `specs/open-ontology-v0.1-plan.md` is still unchecked: CDC-D03 / parent SP04-W04. Author records read: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a294/SP04/cdc-quarantine/` `REPORT.md`, `FINDINGS.md`, and `HANDOFF.md`, plus `CONTRACT.md` and the crate source. Contract `cdc-quarantine-v1`.

## Recipe

Origins are in `recipe-origin.txt`.

- a204 commit `b2b3adabd05de94723d8ed13b33be0144a0de1f9`, preoverlay launch `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841`, inside `b7ee6d634a279b61b49badc1f497fd1a181fdae49d4e21f0fe5f2dec4db87636`, overlay launch `78c8ef8c179c59317850c1b6a9ae53d3f883b8a1b24c2567fa0d3d26f1997eee`, manifest launch `9b28b43d9a9afe9cad69a5f086d7d96af4758f58c7e053a25efa52567a69cdd8`.
- a199 `tools/sha256_manifest.rs` copied byte-identical, sha256 `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`.
- a200 smoke launch `837d1b5bd28cbabf84e92a3295d90c99402029f421942fe1bacee8471be19163` recorded and not applied. Zero PostgreSQL.

Deltas: new builder 6 GiB and 1 CPU, `CARGO_BUILD_JOBS=1`, global rust-builder cap 10, owner a303, fixed archive of `tools/cdc-quarantine` plus root `Cargo.lock` and `rust-toolchain.toml`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `--pull never`, `--platform linux/arm64`, `--cap-drop ALL`, `--security-opt no-new-privileges`, `--memory 6g`, `--memory-swap 6g`, `--cpus 1`. No privileged mode, host network, docker socket, host home, or shared cargo cache. Private volumes `oo-a303-eef831aa-cargo` and `oo-a303-eef831aa-target`. Network `oo-a303-eef831aa-net` for the test runs. Hash runs use `--network none`.

`rust-toolchain.toml` at the fixed commit says channel `1.98.1`. Every container set `RUSTUP_TOOLCHAIN=1.94.0`. `rustc -vV` reported `1.94.0` (`4a4ef493e3a1488c6e321570238084b38948f6db`), host `aarch64-unknown-linux-gnu`. That matches `Cargo.toml` `rust-version`.

## Preoverlay

`linux/preoverlay`, container `37fa234fa96aa08aa1d78f33372c3fe20a3e3929d58fbebd62d9a0049634f1b0`. Hostconfig: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a303-eef831aa-net`, CapDrop `["ALL"]`, Memory `6442450944`, NanoCpus `1000000000`.

`cargo test --locked --offline --all-targets -- --test-threads=1` after an online fetch. `fetch-offline.exit` is 101 on the empty volume. `fetch.exit` is 0. Test count `lines=3 passed=15 failed=0 bad=0`: library `sha256::tests::known_answers` (1), the bin target (0), and 14 tests in `tests/proof.rs`. `clippy.exit` is 0. `lock-subset.exit` is 0, `registry=11`. The 13 crate files matched `source/blobs`.

## Review tests

`overlay/review_proof.rs` is copied into a disposable `git archive` only. The product tree was not edited.

| Directory | docker-run.exit | test.exit | clippy.exit | Container |
| --- | --- | --- | --- | --- |
| `linux/overlay` | 101 | 101 | 101 | `ceaa1023c26b11693e238d113c1cb0dd25a23c73754911b3bf67e150912e7c40` |
| `linux/overlay-2` | 101 | 101 | 101 | `093997edd0df1eb2b84500d64e1e6df76790a0cb2954141cefdc2c08e3925401` |
| `linux/overlay-3` | 101 | 0 | 101 | `b96f565492bdd1b496fe4d9f19a181559335440b163046b220c2b63a870ba203` |
| `linux/overlay-4` | 0 | 0 | 0 | `ad94aa395b01669ab6fc3692abb4672f701c44c60c0d097076ba81540b419651` |

Each of those hostconfigs has the same memory, CPU, capability, privileged, and OOM fields as preoverlay. Failed sources are `linux/overlay/review_proof.failed.rs`, `linux/overlay-2/review_proof.failed.rs`, and `linux/overlay-3/review_proof.failed.rs`.

`linux/overlay-4` ran `cargo test --locked --offline --test review_proof -- --test-threads=1` and `cargo clippy --locked --offline --all-targets -- -D warnings`. Eight tests passed, 0 failed, finished in 0.13s. `fetch-offline.exit` is 0. Clippy finished in 0.12s.

The eight tests are `oracle_known_answers_and_position_is_decimal_text`, `api_and_cli_bind_bytes_and_union_paused_collections`, `exact_replay_and_conflict_keep_stored_bytes`, `counter_quota_is_not_an_os_quota`, `record_cap_does_not_persist_the_refused_event`, `ordinal_and_lsn_are_not_fields_of_this_contract`, `sigkill_checkpoints_recover_incomplete_without_ack`, and `damaged_manifest_receipt_and_hash_are_not_repaired`.

## What the passing run observed

`schema_changed` and `truncate` both seal. Payload bytes, manifest bytes, `STORE`, and `RECEIPT` match the review oracle. The review SHA-256 agrees with the product SHA-256 on the stored payload. Identity is the SHA-256 of the four-field preimage. Manifest `local_prerequisite` is false. Status `local_prerequisite` is true only when `durable_quarantined` is true, `incomplete` is false, and `fault` is absent. `ack_eligible`, `ack_allowed`, `schema_applied`, `target_truncated`, `cross_table_consistent`, `debezium_router`, and `upstream_commit_verified` stay false. `target_applied` stays null. Paused collections recover as the sorted union `public.lines`, `public.orders`.

Exact replay exits 0 and leaves the directory bytes unchanged. A different payload for the same identity exits 2 with `payload_conflict` and leaves those bytes unchanged.

`Limits.max_disk_bytes = 0` and `CDC_QUARANTINE_MAX_DISK_BYTES=0` return `disk_quota`, exit 2, and do not create the store. The same test then writes `os-accepted` with `std::fs::write`. `max_events` 0 and a second event above `max_events` 1 return `record_cap` and do not keep the refused event.

`source_position` `18446744168199581248` is 20 decimal digits. `u64` parse fails. Status provenance is `caller_declared`. Added keys `ordinal` and `lsn` make `parse_control` return an error. The CLI fault is `malformed`, `paused` is false, and the store is absent.

Own-child `Child::kill` at `before_data_fsync`, `before_manifest`, `before_rename`, and `before_parent_fsync` waits for mark `waiting\n` and observes Unix signal 9. Recovery exit is 3. The tree stays incomplete and paused. `RECEIPT` is absent. `ack_eligible` and `ack_allowed` stay false. A second recover, replay, and put leave the inventory unchanged. `unknown_incomplete` is true for the two stages that have no readable manifest, and false for `before_rename` and `before_parent_fsync`, which still have a readable manifest partial or final. That split matches `CONTRACT.md`.

The harness, not the product, deletes `manifest.json`, writes `not-json`, deletes `RECEIPT`, and flips one receipt byte. Recover stays exit 3. Put and replay do not restore, delete, or replace those bytes.

The child `Cargo.lock` sha256 is `d701592871d6430df093ca865cf796f83131e51673b1ffa0dcc0a0bc309679df`. `tools/lock_subset.rs` matched these 11 registry packages to the root lock by name, version, and checksum: itoa 1.0.18, memchr 2.8.3, proc-macro2 1.0.107, quote 1.0.47, serde 1.0.229, serde_core 1.0.229, serde_derive 1.0.229, serde_json 1.0.151, syn 3.0.6, unicode-ident 1.0.26, zmij 1.0.23.

## Inventory and scope

The first hash pass writes `source/product.sha256` and `MANIFEST.sha256`. Those outputs are copied to `source/product-pass1.sha256` and `linux/manifest/inventory.sha256` before the close pass. `linux/manifest-close` then rewrites both hash files. Its `docker-run.exit`, `hostconfig.txt`, `console.log`, and `container-removed.txt` are written after that hash. `cleanup.txt` is written after the close pass. Those five files sit outside the final `MANIFEST.sha256`. The tool also skips the output path while it is writing that file.

The ledger, the specs, the root lock, connector manifests, and `tools/cdc-quarantine` were not edited. No dependency was added. a207 remains the connector manifest and root-lock writer. No release was published. This report does not check the CDC-D03 ledger row and does not claim parent acceptance.
