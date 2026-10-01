# Standalone bench/datagen Linux platform a059

This evidence builds and runs `oo-datagen` from `bench/datagen/Cargo.toml` on Linux. The root workspace matrix does not build this package. Rust 1.98.1 and 1.94.0 each ran on linux/arm64 and linux/amd64.

## Identity

| Item | Value |
| --- | --- |
| Evidence session | `eef831aa-dba4-4af2-a78b-541bc9e0a059` |
| First run | `f1da4b1b-14d9-44e1-acd5-5dadb9ffeac7` |
| Completion run | `3c91753b-5703-4d60-b1ee-917714ec882c` |
| Scope | `sp00-datagen-platform` |
| Project / owner | `open-ontology` / `env` |
| First driver shell | pid 95312, process-exit file absent |
| First driver stop | stdout ends at `start 1.94.0-amd64`; the process was gone at inspection |
| Completion outer exit | `1` |
| Completion cleanup exit | `0` |
| Byte-compare exit | `0` |

## Source

Hashes were taken before the first container and again after the completion run. The snapshot used for every recipe is the sealed 71-file copy, tree `dfc4ba2ae7915369a7bf2075ddef36c3e10139c3cc2926fae98fedcdaf2030c4`. It contains `bench/datagen`, `crates/oo-core`, the root `Cargo.toml`, and `rust-toolchain.toml`. The root `Cargo.lock` and xtask are outside that snapshot. xtask is not a bench dependency.

| Tree | Expected | After | Inventory unchanged |
| --- | --- | --- | --- |
| bench/datagen | `0d2314de4a26fe93ad9d359790b9189c5a3f7c5c705828a98b55be204234c208` | `0d2314de4a26fe93ad9d359790b9189c5a3f7c5c705828a98b55be204234c208` | True |
| oo-core | `585030119865bccc6ca29adbe8e644abc7dd8a99e2aec1b0626059fa9e37b1a8` | `585030119865bccc6ca29adbe8e644abc7dd8a99e2aec1b0626059fa9e37b1a8` | True |
| root Cargo.toml | `b18a3b53f594945296a440ceecebaf702bc0fb26605f2c8d4509feafa4ed1a5c` | `b18a3b53f594945296a440ceecebaf702bc0fb26605f2c8d4509feafa4ed1a5c` | |
| root Cargo.lock | `636e19b8761bd4f212381fd0abe6a3040ca6ee8024aed06dd350a1fd55ec32d1` | `636e19b8761bd4f212381fd0abe6a3040ca6ee8024aed06dd350a1fd55ec32d1` | |
| bench Cargo.lock | `8fe0a83684a72239e116339349a44dada871ba3523f5ca718871c5690faa6df5` | `8fe0a83684a72239e116339349a44dada871ba3523f5ca718871c5690faa6df5` | |
| bench Cargo.toml | `fad63523cdf34234a0c21e8f7270c86e3b1ba103cdc3dfacac4dfb1f9d85dcf8` | `fad63523cdf34234a0c21e8f7270c86e3b1ba103cdc3dfacac4dfb1f9d85dcf8` | |
| rust-toolchain.toml | `7553618f800829444096beea3dce84c3d07585efda504c19adb6787369bbd9e8` | `7553618f800829444096beea3dce84c3d07585efda504c19adb6787369bbd9e8` | |

File counts after the run: datagen 32, oo-core 37. Direct edges recorded on the finished platforms are `oo-core v0.1.0` and `sha2 v0.10.9`.

The After column is the hash written when completion run `3c91753b-5703-4d60-b1ee-917714ec882c` finished. This finalization does not re-read the worktree and does not claim that the root lock still has that value.

## Platforms that finished in the first run

Each of these containers exited 0. Wrapper, wait, and inspect exits are 0. Offline download lines after the fetch marker are 0. The original limits checker wrote exit 1 because it required `/tmp` in `Mounts`. Inspect shows `HostConfig.Tmpfs` `/tmp` = `rw,nosuid,nodev,size=2g`, memory and memory+swap 8589934592, pids 4096, CapDrop ALL, SecurityOpt `no-new-privileges`, user `0:0`, runtime runc. Those `limits.exit` files are kept as recorded.

| Recipe | rustc | host | tests | protocol | xs files | s files | wrong-seed content diffs | binary sha256 | container | wrapper | inspect | limits file |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `1.98.1-arm64` | rustc 1.98.1 (48a229cea 2026-09-01) | host: aarch64-unknown-linux-gnu | test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s; test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s; test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 43.32s; test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s | protocol oo-datagen/v2 | 65 | 65 | content_diffs=29 | `2d37799d4bc00a1e5a13213ed73df245bf2557971f0027c3b88efd7dbea81247` | 0 | 0 | 0 | 1 |
| `1.94.0-arm64` | rustc 1.94.0 (4a4ef493e 2026-03-02) | host: aarch64-unknown-linux-gnu | test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s; test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s; test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 43.69s; test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s | protocol oo-datagen/v2 | 65 | 65 | content_diffs=29 | `be814d7054b3b42fdd311f15e875bdcef871eacf1f0a9a18d8c1fecdac9a13bb` | 0 | 0 | 0 | 1 |
| `1.98.1-amd64` | rustc 1.98.1 (48a229cea 2026-09-01) | host: x86_64-unknown-linux-gnu | test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s; test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s; test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 63.54s; test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s | protocol oo-datagen/v2 | 65 | 65 | content_diffs=29 | `b96b53623e2f209b6898d955e1fd359f4ee260530eab2747780307a74b6a31b7` | 0 | 0 | 0 | 1 |

Cargo test on each finished platform printed four result lines: 17 passed, 0 passed, 3 passed, 0 passed. The 17 lines are the library tests, including numeric, date, time, canonical JSON, DDL, and value fixtures. The 3 lines are `carrier_flags_keys_and_ddl`, `sp00_xs_contract`, and `sp00_s_twice`. Clippy `--all-targets -- -D warnings` exited 0. `oo-datagen version` printed `protocol oo-datagen/v2`.

Seed 11 inventories match the a044 lists byte for byte: xs `f3be74e684ed988c153b20bbf9344dbecbe641ecf543f4fa6204e02d25c27190`, s `943c7a6f59bc3b8a693a1815a73ff518125e158fd5795c8ebbefe53d942dfb35`, 65 files each. Repeat generations on the same platform match. Release binary hashes differ by toolchain and architecture. They are recorded and are not a gold image.

wasm32 was not built. `cargo tree -i wasm-bindgen` and `cargo tree -i js-sys` were inspections. Their phase exit is stored and was excluded from the recipe worst-exit on the first three runs. Each finished platform recorded wasm inspection exit 0.

## Terminated 1.94.0 linux/amd64 attempt

This attempt is in the first run archive at `logs/matrix/1.94.0-amd64` and is a termination.

- Component exit: `0`. rustc line: rustc 1.94.0 (4a4ef493e 2026-03-02).
- `cargo fetch --locked` exit: `0`.
- Offline marker was printed. `cargo-build` started at 2026-10-01T19:33:32Z and has no phase exit file.
- Independent inspect of `oo-f1da4b1b-14d9-44e1-acd5-5dadb9ffeac7-build-1_94_0-amd64`: exit 143, OOMKilled false, started 2026-10-01T19:33:25Z, finished 2026-10-01T19:34:17Z.
- The driver did not write wrapper, wait, inspect, or limits files for this attempt. The archive records `driver_not_written` for those names.
- Later phases, including tests and dataset generation, did not start.

## Completion run 1.94.0 linux/amd64

Run `3c91753b-5703-4d60-b1ee-917714ec882c` used the same sealed snapshot, the same image pin `sha256:f747a9f2b58b8c5f9d6d0680fe8af2edba26ac258a7d6142bb4f65f9693711f4`, linux/amd64, 4 CPUs, 2 cargo jobs, 8 GiB memory, 8 GiB memory+swap, pids 4096, CapDrop ALL, no-new-privileges, user 0:0, `--pull never`. The limits record for this run counts `HostConfig.Tmpfs` `/tmp` as the tmpfs that the first checker looked for under `Mounts`.

| Exit | Value |
| --- | --- |
| container | 0 |
| wait | 0 |
| inspect | 0 |
| wrapper | 0 |
| limits | 0 |
| recipe | 0 |
| offline console lines after marker | 0 |
| offline file | 0 |

| Phase | Exit |
| --- | --- |
| `component` | 0 |
| `cargo-fetch` | 0 |
| `cargo-build` | 0 |
| `cargo-test` | 0 |
| `cargo-clippy` | 0 |
| `cargo-release` | 0 |
| `direct-deps` | 0 |
| `wasm-tree` | 0 |
| `version` | 0 |
| `binary-sha` | 0 |
| `generate-xs-a` | 0 |
| `verify-xs-a` | 0 |
| `manifest-xs-a` | 0 |
| `generate-xs-b` | 0 |
| `verify-xs-b` | 0 |
| `manifest-xs-b` | 0 |
| `repeat-xs` | 0 |
| `baseline-xs` | 0 |
| `baseline-xs-b` | 0 |
| `generate-s-a` | 0 |
| `verify-s-a` | 0 |
| `manifest-s-a` | 0 |
| `generate-s-b` | 0 |
| `verify-s-b` | 0 |
| `manifest-s-b` | 0 |
| `repeat-s` | 0 |
| `baseline-s` | 0 |
| `baseline-s-b` | 0 |
| `generate-xs-12` | 0 |
| `verify-xs-12` | 0 |
| `wrong-seed` | 0 |

rustc: rustc 1.94.0 (4a4ef493e 2026-03-02)
host: host: x86_64-unknown-linux-gnu
tests: test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s; test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s; test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 73.33s; test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
binary: 02cf237e695c76db03187969b980b6ad44fa9b99215b025b673ee97fa622116a  /target/release/oo-datagen
4199888 /target/release/oo-datagen

## Byte comparison

Exit `0`.

Seed 11 inventories from all four platforms match the a044 65-file lists and match each other. Repeats match inside each platform. Seed 12 changes non-manifest bytes on every platform. Seed 12 inventories match across the four platforms.

## Resources and cleanup

The completion pass removed containers, volumes, and the network whose labels are `oo.project=open-ontology`, `oo.owner=env`, `oo.scope=sp00-datagen-platform`, and whose run id is the first run or the completion run. The 11 pre-existing anonymous volumes, a058 labeled objects, the a060 network, and any anonymous volume that this session did not mount were left in place. Images were not pulled and were not pruned.

```
preserve_lost=[]
a039_old_missing=[]
before_count=27
after_count=12
cleanup_exit=0
```

Volume sizes, when the du container ran, are in the completion run `raw/volume-sizes.txt`.

## Bounds

This run does not sign the 32 smells, the 300-question benchmark, Metric D, scales m or l, a wasm32 build, or an air-gap claim. Fetch used the network. Offline applied to build, test, clippy, release, and generation after the marker. It does not close SP00, W04, AT-EVAL-01, or SP12, and it does not certify a harness driver. The root Linux matrix remains a separate result.

## Outer exit 1

`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a059/SP00/datagen-platform/raw/outer.exit` remains `1`. `raw/completion-process.exit` remains `1`. Those files are the completion bookkeeping result. They are separate from the recipe exits.

The completion recipe record has `recipe_exit` 0, `phase_problems` `[]`, byte-compare exit 0, and cleanup exit 0. `scripts/complete-fourth.py` line 541 set `recipe_exit` from the wrapper and limits exits, both 0. Lines 1104–1106 then did this:

```python
outer = 0
if int(recipe.get("recipe_exit") or 1) != 0:
    outer = int(recipe.get("recipe_exit") or 1)
```

`recipe_exit` was 0. In Python `0 or 1` is 1, so the condition was true and `outer` became 1. Line 1111 stored that value, and line 1057 wrote it to `raw/outer.exit`. Line 1164 returned `int(recipe.get("outer_exit") or 1)`, which is the process exit 1 in `raw/completion-process.exit`. The same `or 1` would also turn a stored 0 into process exit 1. No platform phase was missing on the completion archive, and this finalization does not change the stored 1.
