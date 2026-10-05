# Role-pack bundle publication

Session `eef831aa-dba4-4af2-a78b-541bc9e0a221`. One Rust builder. No PostgreSQL. No subagents.

Fixed archive `c2db9c430b822a824a1062b6c3c7345924df7dd8`, parent `cc3bb808c058cb66b088a76dd01725b918514909`, tree `8fb187147b02d6cacf45cb5a683859f698b497cf`. That full SHA was saved in `prework/fixed-sha.txt` before the repair. `d20944017bda07ae8a7e95c8acfcfaebb829aa4a` is an ancestor of that archive.

The repair target is `d20944017bda07ae8a7e95c8acfcfaebb829aa4a`. At `d209`, at the fixed archive, and at `ac76d5e63a58de2253b4c728dffefaf2e0565601`, the three owned paths were the same blobs:

- `crates/oo-builder/src/bin/role-pack-bundle.rs` blob `80c9e915ebe5c7048d15eb4b34c0d3f697c90455`, content SHA-256 `5d5d91b96c0a4fc9fcdfddc9fdb135d19d38b4680d5ac62f07b2fffb8dff573b`
- `crates/oo-builder/tests/role_pack_bundle.rs` blob `ddd0f588b2298eb4e84325d38f7af393a64a58eb`, content SHA-256 `c71051eb2c816890fa85c2c30e2d3abbbc4136e351a8bdccff1e2a7f9c4d7c91`
- `contracts/builder/role-pack-bundle-v1.md` blob `ed645185db5f27802c5f84eb88ed911da7a3fbc1`, content SHA-256 `88bff769b133a84854d7b71fb85b2a6211c65a01d917e1dd676ce5dc1a0a5347`

Product commit `0c6e6f1e0efe419e4ed1d34444dd5b3a332074d0`, parent `ac76d5e63a58de2253b4c728dffefaf2e0565601`, tree `23cb0eaccf51496ec73844421050acc838f17fd0`. Author `Redux0223 <171701490+Redux0223@users.noreply.github.com>`. Subject `builder: publish the role-pack bundle with a non-replacing hard link`. The parent subject is `evidence: review the cancel reservation owner`. The commit changes only the three owned paths: 247 insertions, 32 deletions.

Blobs and content SHA-256 at that commit, also the three overlay lines in `linux/proof/expect-sha256.txt` and the copies in `source/blobs/`:

- `f90ace2010efdd222db85d681b294428e2156100` `ee085c9d1a28288efe700a58e772377a909613f04731aed4af3ce25591cc0bf2` `role-pack-bundle.rs` (35883 bytes)
- `a6558dd1cbf1b52dd652a394a184a154c7b0c91e` `1143c117e86c1373aa825b2571cbf6473b57b65928c4a1c0a830a28cee7e3f39` `role_pack_bundle.rs` (38299 bytes)
- `8aba171993c3c1f6fe3c139d7f02c19907579558` `164388a8e496fd989edcc86f5323257d1863a4d9c7f8058304f828a4b91a2863` `role-pack-bundle-v1.md` (12458 bytes)

`git diff` from the product commit to the worktree is empty for those three paths. No library, existing binary, shared module, manifest, or lock was edited. The development ledger was not edited. This repair does not accept SP07-W06 or O-D02 and does not add a component.

`role-pack-bundle` still reads one `oo.builder.role-pack-command/v1` file, calls `compile_role_pack_command`, and writes one new local file. The bundle splice is unchanged. Publication of that file is `std::fs::hard_link` from the command's own synced staging file to the destination in the same directory.

## Toolchain

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, already local, `linux/arm64`. `docker run --pull never`. `RUSTUP_TOOLCHAIN=1.94.0` overrides `rust-toolchain.toml` channel `1.98.1`. Observed compiler: `rustc 1.94.0 (4a4ef493e 2026-03-02)`, `cargo 1.94.0 (85eff7c80 2026-01-15)`, host `aarch64-unknown-linux-gnu`, Debian 13.4 (trixie), Linux `6.12.76-linuxkit`.

Each product run used one private bridge `oo-a221-eef831aa-net`, `--cap-drop ALL`, `--security-opt no-new-privileges`, `--memory 6g`, `--memory-swap 6g`, `--cpus 1`, `CARGO_BUILD_JOBS=1`, no host network, no socket, no host home, and no shared cargo cache. Cargo home and target were `oo-a221-eef831aa-cargo` and `oo-a221-eef831aa-target`. `/tmp` was `tmpfs` `rw,nosuid,nodev,noexec,size=1g`. The source mount was read-only. Host config recorded `Memory=6442450944`, `NanoCpus=1000000000`, `Privileged=false`, `OOMKilled=false`, `CapDrop=["ALL"]`.

The PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb` was not started.

Recipe origin is `recipes/ORIGIN.md`. The launch shape is the a204 no-PostgreSQL preoverlay pair, with memory 6GiB and 1 CPU. The a200 PostgreSQL launch is recorded there and was not executed. The inventory tool is the a199 std-only hasher, copied with no edits. Its content SHA-256 is `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`.

## Pre-edit smoke

`linux/preoverlay` archived `c2db9c430b822a824a1062b6c3c7345924df7dd8` before any product edit. `repo-head.txt` is `a863ab06796d6169b455fa15f805a2b21c9a0d2c`. `frozen-present.exit` 0. `frozen-bundle-check.exit` 0. `hash-compare.exit` 0. Capacity at launch: `own=0 global=2 host_cargo=0 cap_global=10`. `containers-before.txt` lists `oo-a217-smoke`, `oo-a218-smoke`, and `oo-a218-pg`. They were left alone.

`fetch-offline.exit` 101 is the cold private cargo volume. The same script then ran `cargo fetch --locked` and recorded `fetch.exit` 0. `test.exit` 0. Compile of the test profile finished in 54.05s. The binary tests passed 7 in 0.05s, including `rename_failure_removes_only_the_staging_file`. The process tests passed 8 in 2.11s. `clippy-component.exit` 0. `clippy.exit` 0. Clippy finished in 17.80s and `clippy.stdout` is empty.

The gap oracle is `race_red.rs`. `race-compile.exit` 0. `race.exit` 1. `race-result.txt` records `result=overwritten`, `dest_bytes=BUNDLE-a221-gap`, `staging_exists=false`, `neighbor_bytes=KEEP-NEIGHBOR`. The smoke script requires that exit and that result line, so `docker-run.exit` is 0 and `console.log` ends `worst_phase_exit=0`. Container `5ac4b17533ea4c1f5c89a0c03d8df143a069060fbb54862ad428626746a6671e`, removed by the launch script as `oo-a221-smoke`.

## Proof

The green run is `linux/proof`. It is a fresh `git archive` of `c2db9c430b822a824a1062b6c3c7345924df7dd8` plus the three owned files. `repo-head.txt` is `ac76d5e63a58de2253b4c728dffefaf2e0565601`. `overlay-present.exit` 0. `hash-compare.exit` 0. `no-rename.exit` 0: the script found no `rename(` in the binary and found `std::fs::hard_link(`. `overlay-diff.stdout` lists the three owned paths as `FAILED` against the frozen content pin. `overlay-diff.stderr` is `shasum: WARNING: 3 computed checksums did NOT match`. That warning is the difference check. The launch continued.

`fetch-offline.exit` 0 on the warm volume. `fetch.exit` 0. `clippy-component.exit` 0. Capacity at launch: `own=0 global=0 host_cargo=0 cap_global=10`. `containers-before.txt` lists `oo-a218-pg`. It was left alone.

`cargo test --locked --offline -p oo-builder --bin role-pack-bundle --test role_pack_bundle` exited 0. Compile of the test profile finished in 3.58s. The binary tests passed 11 in 0.00s: `cleanup_refuses_a_directory`, `existing_name_is_not_replaced`, `field_span_keeps_braces_inside_strings`, `hard_link_shares_the_inode_and_then_drops_the_staging_name`, `hash_bytes_matches_sha256_vectors`, `link_errors_use_static_sentences`, `link_failure_removes_only_the_staging_file`, `publish_leaves_a_preexisting_staging_path`, `publish_removes_staging_when_destination_exists`, `readback_mismatch_removes_only_the_staging_file`, `symlink_destination_is_not_replaced`. The process tests passed 10 in 2.15s, including `competing_processes_one_publishes_and_the_other_refuses` and `second_publish_keeps_the_first_bundle`.

`cargo clippy --locked --offline -p oo-builder --bin role-pack-bundle --test role_pack_bundle -- -D warnings` exited 0. Clippy finished in 0.26s and `clippy.stdout` is empty. `docker-run.exit` 0. Container `37b0d87e3acb7eb2916971bc66cf214ed3cf40e374d3d074d8c852f9dff87ba6`, removed by the launch script as `oo-a221-proof`.

The process tests spawn `role-pack-bundle` with `env_clear`. The two-process test writes clerk and audit reference bundles first, then starts both processes against `shared.json`. Exactly one exits 0. The shared file equals that winner's reference bytes. The loser exits 1. Its stderr is one line, either `io.collision` / `the staging path already exists` or `io.destination` / `the output path already exists`. The transcript records `ok` and does not record which role won or which of those two lines was printed. `ada177` and `aud177` are absent from that stderr. The staging name is absent. The neighbor `KEEP-NEIGHBOR` and both reference files stay. `second_publish_keeps_the_first_bundle` is the sequential case: the second call exits `io.destination` and the first bundle bytes stay.

## Preserved non-zero exits

No product compile, test, or clippy failure was produced. There is no `attempt` directory.

| Path | Exit | Meaning |
|---|---:|---|
| `linux/preoverlay/fetch-offline.exit` | 101 | Cold private cargo volume, then online `cargo fetch --locked` exited 0. |
| `linux/preoverlay/race.exit` | 1 | Expected red. The sentinel was replaced. The smoke harness requires this exit together with `result=overwritten`. |
| `linux/proof/overlay-diff.stderr` | warning | `shasum` reported that the three overlay files differ from the frozen pin. The proof launch exited 0. |

## Inventory

`MANIFEST.sha256` is written by `tools/sha256_manifest.rs` after this report, `FINDINGS.md`, `HANDOFF.md`, `CLEANUP.txt`, and `source/blobs/` exist. The tool skips its own output path. Host `rustc 1.98.1 (48a229cea 2026-09-01)` compiles that file. It implements SHA-256 itself and checks the empty digest and the `abc` digest before walking the tree. That run is not a container. Its stdout is `inventory.stdout`.
