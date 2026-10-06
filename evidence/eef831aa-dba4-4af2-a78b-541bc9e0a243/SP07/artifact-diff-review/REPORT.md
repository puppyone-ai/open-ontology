# Report

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a243`. Fixed source `701f80aa6f2089a865d2ff99b5aa5c1bcda1de6a`, tree `9c9a3b71193cb9b02ff10f9b55d7bb3d61d4ea83`. Parent archive for the smoke was `1a1425c9c57924045e8e405c305c76f0a1c08406`, tree `1c59e268599f911c29a6b488952243b9ad243a7c`. Repo HEAD when every container launched was `1ddbd865fc77c7a1a6209949336f592107d77683`. Date: 2026-10-06.

This is an independent review of `ontology-artifact-diff` on that fixed commit. The author record in `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a213/SP07/artifact-diff/` is not this verdict. O-D01 stays unaccepted. SP07-W05, authentication, and publication stay open. The diff document keeps `auto_accept` false, `publication` unavailable, `certification` none, and `grants` not_authorized.

The decisive inputs are sealed command files produced by the `oo-builder` CLI (`captured-pg-draft` and `review-pg-draft`) from a ledger catalog, then passed to the `ontology-artifact-diff` CLI. PostgreSQL was not started. No model was called. No product, library, root, manifest, or lock file was edited.

## What was read

- `contracts/builder/artifact-diff-v1.md` at the fixed tree
- `crates/oo-builder/src/bin/ontology-artifact-diff.rs` and `crates/oo-builder/tests/ontology_artifact_diff.rs` at `701f80aa`
- `crates/oo-builder/src/bin/oo-builder.rs`, `captured_pg.rs`, `pgsql.rs`, `draft.rs`, and `canonical.rs` for the CLI artifact shape and the `pg-catalog-hash/v1` preimage
- a213 `REPORT.md`, `FINDINGS.md`, and `HANDOFF.md`
- a204 launch `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh`, commit `b2b3adabd05de94723d8ed13b33be0144a0de1f9`, sha256 `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841`
- a199 manifest tool, commit `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9`, sha256 `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`
- a200 proof launch, commit `a587031c719a484830d78e493605f48e2056cae4`, sha256 `c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541`. It was not executed. It requires a private PostgreSQL container.
- a213 overlay-3 launch sha256 `f9ea835d4117723f1a95654ee23439ce135857d4e5b67cef8831c8db45ccc36c`, used as the prior adaptation of a204. Its volumes, containers, and logs were not reused.

`recipe-origin.txt` records those origins. The a243 scripts change the UUID, the 6GiB and 1 CPU limits, the archive commit, the included `examples/` path, and the copy of the review oracle onto the ephemeral archive.

## Fixed blobs

`linux/overlay-3/overlay-sha256.txt` matches the saved blobs and the archive of `701f80aa`:

- `ontology-artifact-diff.rs` `ce0ff74acdd8672f6826e33da2d4cb2a4f53f0fcff4d9c57a16d96333548fb4e`
- `ontology_artifact_diff.rs` `857205c2251e837be44b0b9ce517f218fcebcd96804295182e816b0842677fc0`
- contract `a139649d0c19f83e040046bc76ecc7a110b5aed873b96113dcc0e26bbb8f9d6e`
- review oracle on that archive `a02b18507a65ed0997909eb79684d029e382a7a4cc3c2c166dc5190ffd4f4fc7`

The oracle is `oracle/ontology_artifact_diff_a243_review.rs`. It is copied onto the archive as `crates/oo-builder/tests/ontology_artifact_diff_a243_review.rs` and is not part of `701f80aa`.

## Pre-edit smoke

`linux/preoverlay` is a `git archive` of parent `1a1425c9` including `examples/`. The diff command, the author test, and the review oracle are absent (`overlay-absent.exit` 0). The contract blob matches. Clippy was not run on this smoke.

`fetch-offline.exit` 101 on the cold cargo volume. One online `cargo fetch --locked` then recorded `fetch.exit` 0. `test.exit` 0. `bin.exit` 0. `docker-run.exit` 0.

`test.stdout`: `ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s` for `--test captured_pg`. The `oo-builder` binary build finished after that.

Rust `1.94.0` (`4a4ef493e 2026-03-02`), host `aarch64-unknown-linux-gnu`, Debian 13.4. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `RUSTUP_TOOLCHAIN=1.94.0` overrides `rust-toolchain.toml` channel `1.98.1`. Container `b313e2a41fc8868824facec65679ac26577072f61cc3ba05f45b4db12602b23c`. Memory `6442450944`. `NanoCpus=1000000000`. `CapDrop=["ALL"]`. `OOMKilled=false`. `Privileged=false`. Network `oo-a243-eef831aa-net`. `capacity.txt`: `own=0 global=0 host_cargo=0 cap_global=10`. The container was removed.

## Decisive run

`linux/overlay-3` is a fresh `git archive` of `701f80aa` including `examples/`, plus the review oracle only. `capacity.txt`: `own=0 global=0 host_cargo=0 cap_global=10`. `fetch-offline.exit` 0. `fetch.exit` 0. `overlay-present.exit` 0. `test.exit` 0. `review.exit` 0. `clippy-component.exit` 0. `clippy-bin.exit` 0. `clippy-test.exit` 0. `clippy-review.exit` 0. `clippy.exit` 0. `docker-run.exit` 0.

`test.stdout`: `ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.53s`.

`review.stdout`: `ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.42s`.

Container `6d885f86ac5421c0df09d0b1656535616d4b87531f40d2ce3a43a20c694b67ef`. Same image, 6GiB, 1 CPU, cap drop, and private network. `/src` was read-only. `tmpfs /tmp` is `rw,nosuid,nodev,noexec,size=1g`. The container was removed.

Clippy is three commands because a combined `--bin --test` invocation does not define `CARGO_BIN_EXE`:

```text
cargo clippy --locked --offline -p oo-builder --bin ontology-artifact-diff -- -D warnings
cargo clippy --locked --offline -p oo-builder --test ontology_artifact_diff -- -D warnings
cargo clippy --locked --offline -p oo-builder --test ontology_artifact_diff_a243_review -- -D warnings
```

`clippy-bin.stderr`, `clippy-test.stderr`, and `clippy-review.stderr` end with `Finished dev profile`. No warning text.

The ledger catalog is `source_id` `ledger_src`, database `ledger`, tables `public.account` and `public.entry`, foreign key `entry_account_fk`. It is not the author shop catalog. The oracle computes `pg-catalog-hash/v1` itself. Empty `{"tables":[]}` and the published one-column customer preimage stay locked. Ledger mutations of the column name, type, foreign-key width, and nullability change the hash. Reversing the table array does not.

Observed through the CLIs on that ledger:

| Case | Result |
|---|---|
| Two `captured-pg-draft` runs, table order reversed | command documents equal; column comment absent from the draft; owner absent from the wrapper |
| Identical files, pretty JSON, and a reordered command object | same sealed diff bytes, `facts` empty, `unchanged` greater than 0, `schema_hash_equal` true, `auto_accept` false |
| Column `code` renamed to `reference_code` | physical and generated `code` removed, `reference_code` added, source changed, `schema_hash_equal` false |
| `code` `type_oid` 25 to 1043, `text` to `varchar` | physical and generated column changed, `type_reason` `known_builtin_not_converted` |
| FK `entry_account_fk` shrinks from 2 pairs to 1 | id `foreign_key/public.entry/entry_account_fk` stays, physical and generated changed |
| `not_null` false to true, curated classification, decision bound to the before schema hash | physical column changed, generated column stale, curated classification added with `unknown` false, `state` `confirmed`, no `by`, `value_sha256` equals the canonical classification object, decision stale with `action` `accept` and the before hash, queue `stale_decision` detail `stale_schema_hash` and `publishable` false, no generated classification fact |
| Usage | exit 2, stdout empty, the four contract lines |
| Missing file, directory, symlink, duplicate key, replaced protocol, zero `draft_hash`, extra command key | exit 1, stdout empty, one static `io.open`, `io.regular`, `json.duplicate_key`, `version.unsupported`, `hash.mismatch`, or `field.unknown` line |

Diff stdout and refusal stderr omit the comment, owner, curator name, curated label, protocol sentinel, and path sentinels. The zero hash is absent from the `hash.mismatch` stderr. Counts equal the facts for `added`, `changed`, `removed`, and `stale`. Facts are sorted by status, kind, id, layer. `diff_hash` is `payload_sha256` of the canonical document with that field removed, and stdout is that canonical JSON with no trailing newline.

The committed author test on the same archive also passed, including the shop rename, type, foreign key, stale decision, and resealed `duplicate.fact` cases. Those tests call `compile_captured_pg_catalog` in process. They corroborate the shop fixture. They are not the independent catalog.

## Preserved failures

`linux/overlay` container `788a59f4d93ca73603a5da554ec045c5df6484e22b0f20519cd0ce3aca588457`. Author `test.exit` 0 (9 passed). `clippy-bin.exit` 0. `clippy-test.exit` 0. The review oracle failed to compile: `E0277` at the archive copy of the oracle, lines 601 and 642, because `{facts}` asked `Vec<serde_json::Value>` to implement `Display`. `review.exit` 101. `clippy-review.exit` 101. `clippy.exit` 101. `docker-run.exit` 101. `OOMKilled=false`. Memory `6442450944`. `NanoCpus=1000000000`.

`linux/overlay-2` container `dbbbf79bf3366c02757da9f8de17161bfae67ace002df004bfd208bb382f5a3f`. Author `test.exit` 0 (9 passed, 0.54s). All three clippy exits 0. The review test compiled and four cases passed. `cli_curated_classification_and_stale_decision_stay_unaccepted` panicked at oracle line 347: `assert!(!after["draft"].to_string().contains(CURATED_LABEL))`. That assertion reads the compiler draft. The curated column label is stored by the structure-draft echo. `assert_quiet` had already accepted the diff stdout. `review.exit` 101. `docker-run.exit` 101. `OOMKilled=false`. Memory `6442450944`. `NanoCpus=1000000000`.

Those directories were not overwritten. `linux/overlay-3` checks the diff stdout for the label and leaves the compiler draft alone.

## Manifest and cleanup

`linux/manifest/launch.sh` compiles `tools/sha256_manifest.rs` with `rustc --edition 2024 -D warnings` inside the pinned image and writes `MANIFEST.sha256`. The first container stdout is copied to `linux/manifest/console.log`. A second container rewrites `MANIFEST.sha256` after `hostconfig.txt`, `console.log`, and the exit files exist. The second stdout stays outside this directory. `/tmp` is `noexec`, so the binary is installed at `/usr/local/bin/sha256_manifest` in the container.

`CLEANUP.txt` records removal of this task's network `oo-a243-eef831aa-net`, volumes `oo-a243-eef831aa-cargo` and `oo-a243-eef831aa-target`, and `/tmp/oo-a243-eef831aa-dba4-4af2-a78b-541bc9e0a243`. The pinned Rust image stays. `docker prune` was not run. Foreign PostgreSQL containers were left in place.
