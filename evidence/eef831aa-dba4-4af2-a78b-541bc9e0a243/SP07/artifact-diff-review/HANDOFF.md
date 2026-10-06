# Handoff

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a243`. Fixed source `701f80aa6f2089a865d2ff99b5aa5c1bcda1de6a`, tree `9c9a3b71193cb9b02ff10f9b55d7bb3d61d4ea83`. Repo HEAD at each launch was `1ddbd865fc77c7a1a6209949336f592107d77683`. This directory is the independent review record. O-D01 stays unaccepted. SP07-W05, authentication, and publication stay open.

The command reads two `oo.builder.captured-pg-draft-command/v1` files and writes one `oo.builder.artifact-diff/v1` object. `auto_accept` is false. `publication` is `unavailable`. `certification` is `none`. `grants` is `not_authorized`. PostgreSQL was not started. No model was called. No product source, library, root, manifest, or lock file was edited by this review.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `source/blobs/ontology-artifact-diff.rs`, `source/blobs/ontology_artifact_diff.rs`, and `source/blobs/artifact-diff-v1.md`
- `oracle/ontology_artifact_diff_a243_review.rs`
- `linux/overlay-3/` for the decisive run, including `review.stdout`, `test.stdout`, `clippy.stderr`, and `hostconfig.txt`
- `linux/preoverlay/` for the parent smoke
- `linux/overlay/` and `linux/overlay-2/` for the preserved oracle failures
- `CLEANUP.txt` and `MANIFEST.sha256`
- `tools/sha256_manifest.rs` and `recipe-origin.txt`

Blob sha256:

- contract `a139649d0c19f83e040046bc76ecc7a110b5aed873b96113dcc0e26bbb8f9d6e`
- command `ce0ff74acdd8672f6826e33da2d4cb2a4f53f0fcff4d9c57a16d96333548fb4e`
- author test `857205c2251e837be44b0b9ce517f218fcebcd96804295182e816b0842677fc0`
- review oracle on the overlay-3 archive `a02b18507a65ed0997909eb79684d029e382a7a4cc3c2c166dc5190ffd4f4fc7`

## Decisive exits

`linux/overlay-3`: `overlay-present.exit` 0, `fetch-offline.exit` 0, `fetch.exit` 0, `test.exit` 0, `review.exit` 0, `clippy-component.exit` 0, `clippy-bin.exit` 0, `clippy-test.exit` 0, `clippy-review.exit` 0, `clippy.exit` 0, `docker-run.exit` 0.

Author `test.stdout`: `ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.53s`.

Review `review.stdout`: `ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.42s`.

Container `6d885f86ac5421c0df09d0b1656535616d4b87531f40d2ce3a43a20c694b67ef`. Memory `6442450944`. `NanoCpus=1000000000`. `CapDrop=["ALL"]`. `Privileged=false`. `OOMKilled=false`. Network `oo-a243-eef831aa-net`. `capacity.txt` recorded `own=0 global=0 host_cargo=0`.

## Preserved attempts

- `linux/preoverlay`: archive of `1a1425c9c57924045e8e405c305c76f0a1c08406` with `examples/`. `captured_pg` 6 passed, 0.09s. `oo-builder` bin exit 0. `fetch-offline.exit` 101, `fetch.exit` 0, `docker-run.exit` 0. Container `b313e2a41fc8868824facec65679ac26577072f61cc3ba05f45b4db12602b23c`.
- `linux/overlay`: review oracle `E0277` on `{facts}` display. Author test 9 passed. Product clippy 0. `review.exit` 101, `clippy.exit` 101, `docker-run.exit` 101. Container `788a59f4d93ca73603a5da554ec045c5df6484e22b0f20519cd0ce3aca588457`.
- `linux/overlay-2`: review test panicked because the compiler draft still contains the curated column label. Diff stdout had already omitted it. Author test 9 passed. Clippy 0. `review.exit` 101, `docker-run.exit` 101. Container `dbbbf79bf3366c02757da9f8de17161bfae67ace002df004bfd208bb382f5a3f`.

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `RUSTUP_TOOLCHAIN=1.94.0`, archive of `701f80aa6f2089a865d2ff99b5aa5c1bcda1de6a` including `examples/`, private cargo and target volumes, private network, `HOME=/tmp`, 6GiB, 1 CPU. Copy `oracle/ontology_artifact_diff_a243_review.rs` onto that archive as `crates/oo-builder/tests/ontology_artifact_diff_a243_review.rs`. Install clippy in the same container. No PostgreSQL.

```text
cargo fetch --locked --offline
cargo test --locked --offline -p oo-builder --test ontology_artifact_diff
cargo test --locked --offline -p oo-builder --test ontology_artifact_diff_a243_review
cargo clippy --locked --offline -p oo-builder --bin ontology-artifact-diff -- -D warnings
cargo clippy --locked --offline -p oo-builder --test ontology_artifact_diff -- -D warnings
cargo clippy --locked --offline -p oo-builder --test ontology_artifact_diff_a243_review -- -D warnings
```

Launch origin is a204 `linux/preoverlay/launch.sh` at `b2b3adabd05de94723d8ed13b33be0144a0de1f9`, sha256 `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841`. a200 was not executed.

## Manifest

`tools/sha256_manifest.rs` is the a199 std-only Safe Rust tool, sha256 `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. It checks the empty digest and `abc`, refuses symlinks, and writes `MANIFEST.sha256` for every regular file except that manifest. `linux/manifest/launch.sh` compiles it with `rustc --edition 2024 -D warnings`. The first container's stdout is `linux/manifest/console.log`. A second container rewrites `MANIFEST.sha256` after that console, `hostconfig.txt`, and the exit files are closed. The second stdout stays outside this directory.

## Cleanup

`CLEANUP.txt` records removal of this task's container names, network `oo-a243-eef831aa-net`, volumes `oo-a243-eef831aa-cargo` and `oo-a243-eef831aa-target`, and `/tmp/oo-a243-eef831aa-dba4-4af2-a78b-541bc9e0a243`. The pinned Rust image stays. `docker prune` was not run. Foreign PostgreSQL containers were left in place.

Contract commit `1a1425c9c57924045e8e405c305c76f0a1c08406` contains the artifact-diff contract. Rust commit `701f80aa6f2089a865d2ff99b5aa5c1bcda1de6a` contains the command and its integration test. The evidence commit contains only this directory.
