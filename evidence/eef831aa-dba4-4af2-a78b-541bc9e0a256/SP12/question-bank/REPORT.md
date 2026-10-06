# Public question bank proof

Session `eef831aa-dba4-4af2-a78b-541bc9e0a256`. Fixed product archive `c828e2781fc37eb62249fab54fa0b7fff86f0249`, tree `6269034aa3cf91e6ee90112aab9f210b5b545625`. Repo HEAD at the proof was `e447249ab1a7f4c82df4729090b1285c9ef4a4ec`. This is a standalone validator for one public synthetic question bank: a manifest, an agent view, and a scorer view, each with a content hash.

This report does not accept B-D06, SP12-W01, AT-EVAL-01, AT-EVAL-02, the 300-question bank, two-human annotation, the 20% dual-annotation gate, or a release.

## What was built

`benchmarks/question-bank` is its own Cargo workspace. It is not a root workspace member. The root manifest, root lock, and product dependencies were not edited. `oo-core` is a path dependency. The proof compiles that path against a `git archive` of the fixed commit, with the archive's `rust-toolchain.toml` renamed so the image stays on 1.94.0.

The contract was frozen in commit `4fed811233bb4ebd675c2f6ffe0b8a5fb72ab56e` before the validator sources. The standalone lock was created with `cargo generate-lockfile`, not `cargo update`. Every registry name, version, and checksum in that lock is already in the fixed root lock. `linux/impl-5/lock-drift.txt` is empty. The pins are serde 1.0.229 checksum `4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba` and serde_json 1.0.151 checksum `c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14`. The verified lock is `benchmarks/question-bank/Cargo.lock`, copied from the attempt 5 archive after that attempt exited 0. The attempt 5 source list, 26 files, was taken before that lock existed and matches the crate sources that were tested. The lock is the 27th crate file.

## Proof

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, rustc 1.94.0. One CPU, 6 GiB, cap-drop ALL, no new privileges, private network `oo-a256-eef831aa-net`, private cargo and target volumes. No PostgreSQL. Each container was removed after its run. No prune. The host did not run cargo or rustc.

Smoke, before the question-bank sources were archived: `oo-core` test `yaml_and_json_numbers_stay_lexical`, 1 passed, 18 filtered, exit 0. The fresh cargo volume missed the offline fetch (exit 101) and completed the online fetch (exit 0). The question-bank crate was absent. Recorded in `linux/smoke/`. Container `22af2442f7bbc23385adad0a72c07979c0c71bf7b14b3d86097d9a425fafe082`.

Attempts 1, 2, and 4 failed and are preserved. Attempt 3 exited 0 on the sources from before the payload-cap correction. Attempt 5 is the proof of the delivered sources.

| Attempt | Docker exit | Phase exits | What the logs show |
| --- | --- | --- | --- |
| `linux/impl` | 2 | test 101, fmt 1, clippy 101, cli 2 | `E0282` at `src/parse.rs:899` because `columns` was inferred too late. rustfmt wrote a diff. The binary was missing, so the CLI phase exited 2. The launcher keeps the last non-zero phase, which is why docker exit is 2. Container `e72e39c634ab9078122ffd746aab63f89a8420594e80f337b3a5346c3a30c314`. |
| `linux/impl-2` | 101 | test 101, fmt 0, clippy 101, cli 0 | `shape_and_label_failures_use_the_contract_tokens` left `schema`, right `int64`, at `tests/bank.rs:397`. Eight bank tests passed. Clippy `type_complexity` at `src/parse.rs:366`. Container `97468bd05fb4e8aa3760dd027624146626d4a2ea9ca39382ddd25afc3e27b7ce`. |
| `linux/impl-3` | 0 | test 0, fmt 0, clippy 0, cli 0 | 9 bank tests and 3 CLI tests passed. This tree still counted identity bytes in the payload. It is not the delivered source. Container `1ee5bb4f67d4ab62df5031ee3770dfa2d033c7268f81d4e14029c0a53368e5f1`. |
| `linux/impl-4` | 1 | test 0, fmt 1, clippy 0, cli 0 | 10 bank tests passed, including `payload_cap_ignores_identity_bytes`. rustfmt wanted the new `if` wrapped. Container `6d794dc26121dfcde2fa93dc240fec4d702b7ba7b9c88a4f47157de5ff1e1485`. |
| `linux/impl-5` | 0 | test 0, fmt 0, clippy 0, cli 0 | Delivered sources. Container `4cea66f8112871d54565dfd411d552ee2ad5d9e6e2c5e4769f50f2466394e901`. |

Attempt 5 host config: `OOMKilled=false`, `Privileged=false`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`, `NetworkMode=oo-a256-eef831aa-net`. Phase exits for rustc, source hash, lock, lock drift, test, component install, rustfmt, clippy, CLI, and docker are all 0. rustc release is `1.94.0`.

`cargo test --locked --offline -- --test-threads=1`: 10 bank tests passed and 3 CLI tests passed, 0 failed. Doc tests and crate unit tests are empty.

The debug binary then ran the fixtures. `approved` exited 0. `mixed` and `reordered` exited 1. A second `mixed` run matched the first stdout, and the reordered stdout differed. The twelve config fixtures exited 2 with empty stdout and the contract stderr line: duplicate id, missing gold on a fully approved question, bad version, bad as_of, holdout split, customer distribution, relative tolerance, equal role-group answers, duplicate role, leading-zero decimal, human annotator `li`, gold SQL mismatch, and a duplicate JSON key.

## Behavior the tests cover

The mixed bank has five public synthetic questions. Three labels are `fully_approved` and two are `needs_reannotation`. Both unresolved questions stay in the manifest, the agent view, and the scorer view. The agent-view bytes do not contain the four planted canaries. The scorer-view bytes do. Forbidden gold and annotation keys are absent from the agent view.

Content hashes bind the documents. Scale moves only the manifest hash. Gold SQL moves the scorer and manifest hashes. Question text moves the agent hash. Reordering the question array moves all three and matches `reordered.json`. The same input is deterministic, including under key-order and whitespace normalization.

Typed gold keeps coefficient `120` at scale 2. `ExactDecimal` `120` scale 2 rescales exactly to `1200` scale 3, and the stored cell is not rewritten. A non-canonical int64 is `int64`. Negative zero is `decimal`. A payload of 16_384 contract bytes is accepted, and one more text byte is `budget` at `/`. Identity fields are outside that sum.

## Inventory

`tools/sha256_manifest.rs` is the untouched a199 hasher. `linux/inventory/launch.sh` compiles it in the same pinned image and writes `crate-sha256.txt` and `SHA256SUMS.txt`. `linux/inventory/rerun.sh` runs after that launcher has closed its console, hostconfig, and exit files, and rewrites those two manifests. The second stdout stays at `/tmp/oo-a256-eef831aa-dba4-4af2-a78b-541bc9e0a256/hash-again.log` and is not copied into this directory.

## Reviews

Four reviews of the frozen scope are in `review/`. They are the same session, not a second author. The payload defect found against attempt 3 is corrected in the attempt 5 tree. The other three passes did not change the source.
