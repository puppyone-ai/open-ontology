# Handoff

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a237`. Reviewed source `bf30b717029d0715e0a1abb7fc8ec02ab7630534`, tree `9e97f9fc2569ef320fcb2a30693c551c6d731c1c`, parent `543580768dec60d127eb428c09e0a9a400b18272`. Author evidence `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a220/SP05/file-worker-parent` is the implementation record. This directory is the independent review proof. No product file was edited. The parent ledger was not edited.

The approval covers parent supervision of one explicit CSV, TSV, or JSONL pull: framed batches, lineage, clean EOF, and `ParentParseComplete` with import, publication, and `pg_run_complete` false. `KERNEL_RESTRICTION` stays `Unsupported`. A `setsid` grandchild survives the group kill. SP05-W02, AT-FILE-02, and the phase ledger stay open.

Worker approval stays a224. CSV library approval stays a199. JSONL library approval stays a185. This review's fixtures are independent of those suites.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `source/blobs/file_worker_parent.rs` and `source/blobs/file-worker-parent-v1.md`
- `overlay/file_worker_parent_a237_review.rs`
- `linux/overlay-4/` for the decisive run, including `test.stdout` and `oracle/`
- `linux/preoverlay/` for the author smoke before the overlay compiled
- `linux/overlay/`, `linux/overlay-2/`, and `linux/overlay-3/` for preserved harness failures
- `CLEANUP.txt` and `MANIFEST.sha256`
- `tools/sha256_manifest.rs` and `tools/sha256_files.rs`
- `recipes/ORIGIN.txt`

## Decisive exits

`linux/overlay-4`: `blob-cmp.exit` 0, `parent-present.exit` 0, `fetch.exit` 0 (`cargo fetch --locked --offline`), `clippy-component.exit` 0, `test.exit` 0, `hash-rustc.exit` 0, `hash-compare.exit` 0, `clippy.exit` 0, `docker-run.exit` 0.

`test.stdout`: `ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.17s`.

CSV object `5c1f3ff40c4c4e2023639f63d098ddb3a7e86cbfc1df190f534516325364d825`. TSV object `52b23b89cd9d79ef686907f22f0c1b9a12a3e0c0d5f8dba46f82882fbd9ea50b`. JSONL object `6f3bea2e4a200553bde6dca510a4189abcc8cf3a465fcc30fcc8dfb23964f847`. Empty object `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`. Recovery object `d9bae52486aece478d26cc4cb51c47a7345eab837eba6bf47ac6a0a9ad5b4f5b`. `linux/overlay-4/hash-compare.txt` shows `sha256sum` equal to the Rust oracle for each file. Container `c0e34504663b413648d375181ee472ea2269eca544b34c58c602d47d28b73c86`. Memory `6442450944`. `NanoCpus=1000000000`. `CapDrop=["ALL"]`. `OOMKilled=false`. `Privileged=false`.

Stall line: `stall_elapsed_ms=835 kind=TimedOut exit=None signal=Some(9) group=Sent reaped=true alive=false`. Setsid line: `setsid hold_pid=189 session=189 direct_reaped=true direct_alive=false group=Sent kernel=unsupported`, then `setsid cleanup hold_alive=false`.

Clippy for that run is `cargo clippy --locked --offline -p oo-connectors --test file_worker_parent_a237_review -- -D warnings`. Stderr ends `Finished dev profile [unoptimized + debuginfo] target(s) in 0.24s` after two pre-existing `sqlx-postgres` `unfulfilled_lint_expectations` warnings. `clippy.stdout` is empty.

`linux/preoverlay`, before the review harness was in the archive: offline fetch `101` on a cold volume, then `fetch.exit` 0, author `test.exit` 0 (`8 passed`, 1.00s), `clippy.exit` 0, `docker-run.exit` 0. Rust `1.94.0` on `linux/arm64`. Container `0295df92840ec44066887bfb2044e1e7d3483f2cc1d8461604a8ed60ecdf7364`. Memory `6442450944`. `NanoCpus=1000000000`. `CapDrop=["ALL"]`. `OOMKilled=false`. `Privileged=false`.

## Preserved attempts

- `linux/overlay` harness `E0716`. `test.exit` 101. `hash-compare.exit` `not_run`. Container `36320a81d02d1416489df80df841d645122c10db7a40a38d2d4db989a9408666`.
- `linux/overlay-2` harness `E0308`. `test.exit` 101. `hash-compare.exit` `not_run`. Container `da25d89cdac39d9c6ae0315e41b4bc3e3ac3541fc1b9203ac095b47a45b6a9ac`.
- `linux/overlay-3` tests passed and the hash compare matched. Clippy `while_let_loop` and `let_and_return` exited 101. Container `1d86f3510db3029ae3dd74c9acb7b110623fb982a92cbb2f3e8f14b5e6b93a59`.

## Manifest

`tools/sha256_manifest.rs` is std-only Safe Rust, copied unchanged from a199 (`d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`). It checks the empty digest and `abc`, walks this directory, and writes `MANIFEST.sha256` for every regular file except that manifest. `linux/manifest/launch.sh` compiles it with `rustc --edition 2024 -D warnings` to `/usr/local/bin/sha256_manifest`. The binary is not placed on the `noexec` tmpfs. The first container stdout is copied to `linux/manifest/console.log` after that process exits. A rewrite container then runs after `console.log` and `hostconfig.txt` are closed, and its stdout is copied to `linux/manifest/rewrite-console.log`. One later container rewrites `MANIFEST.sha256` after those files are closed and does not write another file into this directory. Its stdout stays outside this directory. Container ids for the in-tree runs are in `linux/manifest/hostconfig.txt` and `linux/manifest/rewrite-hostconfig.txt`.

## Cleanup

`CLEANUP.txt` records removal of network `oo-a237-eef831aa-net`, volumes `oo-a237-eef831aa-cargo` and `oo-a237-eef831aa-target`, and `/tmp/oo-a237-eef831aa-dba4-4af2-a78b-541bc9e0a237`. The launch scripts had already removed the containers. The pinned Rust image stays. `docker prune` was not run. PostgreSQL was not started. Foreign containers were not removed.

## Limits

F-D02, SP05-W02, and AT-FILE-02 stay open. The ledger was not updated. This review makes no parent-acceptance, full-goal, or release claim. The caller kernel admission is `Unsupported`. The setsid observation is the escaped-session limitation, preserved and seen, and it is not a containment certificate.
