# Paired trial statistics proof

Session `eef831aa-dba4-4af2-a78b-541bc9e0a225`. Fixed product archive `c2db9c430b822a824a1062b6c3c7345924df7dd8`. This is the SP12-W03 primitive B-D03: two typed score vectors become exact pass^k counts and one paired question bootstrap report.

This report does not accept B-D03, a question bank, a quality benchmark, or a release.

## What was built

`benchmarks/trial-statistics` is its own Cargo workspace. It is not a root workspace member. The root manifest, root lock, and product dependencies were not edited. `oo-core` is a path dependency. The proof compiles that path against a `git archive` of the fixed commit, with the archive's `rust-toolchain.toml` renamed so the image stays on 1.94.0.

The standalone lock was created with `cargo generate-lockfile`, not `cargo update`. Every registry name, version, and checksum in that lock is already in the fixed root lock. `linux/impl-4/lock-drift.txt` is empty. The pins are serde 1.0.229 checksum `4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba` and serde_json 1.0.151 checksum `c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14`. The verified lock is `benchmarks/trial-statistics/Cargo.lock`, copied from the attempt 4 archive.

## Proof

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, rustc 1.94.0. One CPU, 6 GiB, cap-drop ALL, no new privileges, private network `oo-a225-eef831aa-net`, private cargo and target volumes. No PostgreSQL. Each container was removed after its run. No prune.

Smoke, before the trial-statistics sources were archived: `oo-core` test `yaml_and_json_numbers_stay_lexical`, 1 passed, 18 filtered, exit 0. The fresh cargo volume missed the offline fetch (exit 101) and completed the online fetch (exit 0). Recorded in `linux/smoke/`. Container `e87421ad3ecbe83b4b42d11e3f7ccc7ed939a6606f06c1fcce6f29e777fc0d3c`.

Implementation attempts 1 through 3 are preserved and failed. Attempt 4 is the proof.

| Attempt | Docker exit | Cause |
| --- | --- | --- |
| `linux/impl` | 101 | rustfmt diff; clippy `manual_contains` in `needs_reannotation`. Tests and CLI fixtures exited 0. Container `07d4bc76425dd33150e6f458aec66d5564c466e07b361ed374e7ff5ce50010d0`. |
| `linux/impl-2` | 101 | clippy `same_item_push` and `too_many_arguments` in `tests/bounds.rs`. rustfmt, tests, and CLI fixtures exited 0. Container `a02a446bbd55a1b0194251b44f96832943e22eb0ba1bb7132ea83f05cc84a384`. |
| `linux/impl-3` | 1 | rustfmt wrap of the two `variant(...)` lets. Clippy, tests, and CLI fixtures exited 0. Container `e7a27d64687e113038b5fad144423c179cb15fb645f35f861514d6b524547be6`. |
| `linux/impl-4` | 0 | tests, rustfmt, clippy, and CLI fixtures. Container `495847e884b9fd5f61c94a6f04c343010841aafdf746a6d9e15955c593bf1dd9`. |

Attempt 4 host config: `OOMKilled=false`, `Privileged=false`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`, `NetworkMode=oo-a225-eef831aa-net`. Phase exits for rustc, source hash, lock, lock drift, test, component install, rustfmt, clippy, CLI, and docker are all 0.

`cargo test --locked --offline -- --test-threads=1`: 17 passed, 0 failed. That is 7 bound tests, 3 compiled-CLI tests, 3 enumeration tests, and 4 gold tests. Doc tests and crate unit tests are empty.

The debug binary then compared the three handwritten fixtures. `k3-passed` and `k3-not-pass-at` exited 0. `k3-reannotation` exited 1. A second run of `k3-passed` matched the first stdout. `mixed-tuple`, `missing-trial`, and `duplicate-key` exited 2 with empty stdout and the contract stderr lines. Stderr was empty on the three report fixtures.

The attempt 4 source list, 21 files, matches the crate sources that were tested. The lock file copied into the crate is the lock that attempt 4 generated inside the archive. The source list was taken before that lock existed.

## Behavior the tests cover

pass^k is 1 only when every trial on that question is `pass`. The k=3 not-pass-at fixture has right statuses pass, pass, fail, and the report point is `-1/1`. The gold bytes contain numerator `-1` with denominator `1`, and they do not contain the pass@1 average `-1/3`.

`needs_reannotation` keeps the question in `question_count` and fails the gate. The k=3 reannotation fixture keeps `question_count` 3, sets the left `pass_k_count` to 0, and sets `reannotation_count` to 1. A question that needs reannotation on both variants counts once.

The bootstrap draws each question with its whole trial vector. The same indexes are applied to both variants. SplitMix64 seed 0 matches `e220a8397b1dcdaf`, `6e789e6aa1b965f4`, `06c45d188009454f`, `f88bb8a8724c81ec`. Seed 1 matches `910a2dec89025cc1`. The rejection tail for n of 1, 2, 3, and 256 matches an independent copy. An independent enumeration of 80 resamples from seed 1 produces the same report bytes as the library. Floor ranks for 80 resamples are 2 and 78. Floor ranks for 10000 resamples are 250 and 9750.

Bounds covered here: 256 questions pass and 257 is budget; k of 8 passes and k of 9 or 0 is budget; resamples 10000 passes and 10001, 0, and -1 are budget; `1.5` and `1e3` are schema; the document cap is 1_048_576 and one byte past it is budget before parse; duplicate keys and a non-UTF-8 prefix are json; an unknown key is schema; seed `01`, a value past `u64::MAX`, and `-1` are schema; the max `u64` seed is echoed; status words `error` and `missing` are schema; a tuple field mismatch is `tuple` and stops the comparison; question ids must line up in order; a duplicate question id, a repeated variant id, a bad index, an overlong id, a control byte in a tuple value, and a payload over 65536 are config.

## Inventory

`tools/sha256_manifest.rs` is the untouched a199 hasher. `linux/inventory/launch.sh` compiles it in the same pinned image and writes `crate-sha256.txt` and `SHA256SUMS.txt`. `linux/inventory/rerun.sh` runs after that launcher has closed its console, hostconfig, and exit files, and rewrites those two manifests. The second stdout stays at `/tmp/oo-a225-eef831aa-dba4-4af2-a78b-541bc9e0a225/hash-again.log` and is not copied into this directory.
