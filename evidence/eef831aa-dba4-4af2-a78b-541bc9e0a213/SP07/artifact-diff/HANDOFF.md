# Handoff

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a213`. Fixed source `a661dcdf72ff91561ae6b7cb79916e582884d08a`, tree `0a8f46576d5f4180ca44ce5f33a414e00c923437`. Repo HEAD at the overlay-3 launch was `ca2e248d38e90e6e12743bf607a5793ce6798ac5`. This directory is the implementation and review record for the artifact-diff slice. O-D01 stays unaccepted. SP07-W05, authentication, and publication stay open.

The command reads two `oo.builder.captured-pg-draft-command/v1` files and writes one `oo.builder.artifact-diff/v1` object. `auto_accept` is false. `publication` is `unavailable`. `certification` is `none`. `grants` is `not_authorized`. PostgreSQL was not started. No model was called.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `source/blobs/ontology-artifact-diff.rs`, `source/blobs/ontology_artifact_diff.rs`, and `source/blobs/artifact-diff-v1.md`
- `linux/overlay-3/` for the decisive run, including `test.stdout`, `clippy.stderr`, and `hostconfig.txt`
- `linux/preoverlay/` for the smoke before the command existed
- `linux/overlay/` and `linux/overlay-2/` for the preserved failures
- `CLEANUP.txt` and `MANIFEST.sha256`
- `tools/sha256_manifest.rs`

Blob sha256:

- contract `a139649d0c19f83e040046bc76ecc7a110b5aed873b96113dcc0e26bbb8f9d6e`
- command `ce0ff74acdd8672f6826e33da2d4cb2a4f53f0fcff4d9c57a16d96333548fb4e`
- test `857205c2251e837be44b0b9ce517f218fcebcd96804295182e816b0842677fc0`

## Decisive exits

`linux/overlay-3`: `overlay-present.exit` 0, `fetch-offline.exit` 0, `fetch.exit` 0, `test.exit` 0, `clippy-component.exit` 0, `clippy-bin.exit` 0, `clippy-test.exit` 0, `clippy.exit` 0, `docker-run.exit` 0.

`test.stdout`: `ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.52s`.

Container `5b58e88db76a98dfd7275e95b0e3ef6778a43c51fc4e27c3111535904cff829a`. Memory `6442450944`. `NanoCpus=1000000000`. `CapDrop=["ALL"]`. `Privileged=false`. `OOMKilled=false`. Network `oo-a213-eef831aa-net`. `capacity.txt` recorded `own=0 global=1 host_cargo=0`.

Shop schema `bcb019d92270bf12092a3f47697b25719cc087a1ac4b7c56a10cf6dd00294c4c`. Stale schema `31bbede923a28e58c8fef54240de5458533fa881e4d5898cdd31504b4aa0bd7e`. Identical catalogs: `unchanged` 33 and no facts. Rename: `unchanged` 10. Type change and FK change: `unchanged` 12. Stale decision with curated classification: `unchanged` 12, `auto_accept` false.

## Preserved attempts

- `linux/preoverlay`: archive omitted `examples/`. `lib.exit` 101 at `draft_tests.rs:1792` (`failures/adapter.code` missing). `captured-pg.exit` 0 (`6 passed`, 0.12s). `clippy.exit` 0. `docker-run.exit` 101. Container `723377b98c4ee906e92f5342eebbe2b7456030fbfa764b0825dbe9e6321c04a3`.
- `linux/overlay`: `char` has no `LowerHex` format, and combined clippy did not define `CARGO_BIN_EXE_ontology_artifact_diff`. `test.exit` 101, `clippy.exit` 101, `docker-run.exit` 101. Container `7cb4b63ae517297db1751d3dd9857f4326faf5c409b947180fc6638e5e819770`.
- `linux/overlay-2`: `std::env::var("CARGO_BIN_EXE_ontology_artifact_diff")` was `NotPresent` at test line 908. Clippy rejected nested `if`s, a redundant closure, and `diff_map` with 8 arguments. The preimage test passed. `test.exit` 101, `clippy.exit` 101, `docker-run.exit` 101. Container `36a682cd8996b0d251faeecbcaaab1cb033a75aec2e412eb8d2222ecb7d734ca`.

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `RUSTUP_TOOLCHAIN=1.94.0`, archive of `a661dcdf72ff91561ae6b7cb79916e582884d08a` including `examples/`, private cargo and target volumes, private network, `HOME=/tmp`, 6GiB, 1 CPU. Copy the two blobs onto that archive as `crates/oo-builder/src/bin/ontology-artifact-diff.rs` and `crates/oo-builder/tests/ontology_artifact_diff.rs`. Install clippy in the same container. No PostgreSQL.

```text
cargo fetch --locked --offline
cargo test --locked --offline -p oo-builder --test ontology_artifact_diff
cargo clippy --locked --offline -p oo-builder --bin ontology-artifact-diff -- -D warnings
cargo clippy --locked --offline -p oo-builder --test ontology_artifact_diff -- -D warnings
```

Launch origin is a204 `linux/preoverlay/launch.sh` at `b2b3adabd05de94723d8ed13b33be0144a0de1f9`, sha256 `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841`. The a213 script uses a new evidence directory, 6GiB, 1 CPU, the `examples/` archive path, and the split clippy commands. a200 was not executed.

## Manifest

`tools/sha256_manifest.rs` is the a199 std-only Safe Rust tool, sha256 `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. It checks the empty digest and `abc`, refuses symlinks, and writes `MANIFEST.sha256` for every regular file except that manifest. `linux/manifest/launch.sh` compiles it with `rustc --edition 2024 -D warnings` to `/usr/local/bin/sha256_manifest`. `/tmp` is `noexec`, so the binary is not placed on the tmpfs. The recorded container stdout is `linux/manifest/console.log`. A later run of the same tool rewrites `MANIFEST.sha256` after `console.log`, `hostconfig.txt`, and the exit files are closed. That later stdout stays outside this directory.

## Cleanup

`CLEANUP.txt` records removal of this task's container name, network `oo-a213-eef831aa-net`, volumes `oo-a213-eef831aa-cargo` and `oo-a213-eef831aa-target`, and `/tmp/oo-a213-eef831aa-dba4-4af2-a78b-541bc9e0a213`. The pinned Rust image stays. `docker prune` was not run. Foreign PostgreSQL containers were left in place. PostgreSQL was not started.

Contract commit `1a1425c9c57924045e8e405c305c76f0a1c08406` contains only `contracts/builder/artifact-diff-v1.md`. Rust commit `701f80aa6f2089a865d2ff99b5aa5c1bcda1de6a` contains only the command and its integration test. The evidence commit contains only this directory.
