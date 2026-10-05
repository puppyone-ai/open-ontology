# a215 parser reap review

Verdict: **APPROVE_LIMITED**.

Reviewed commit `c27d6349ffa7cf8629a5a79c62015ca5c255e18f` (`gateway: bound parser teardown to the process group`), parent `90dfac17534983e3f3f93d5f437806193c24095b`, tree `c53545c3ba03dbc0e9da5ddb1ea84916228ec947`, author Redux0223, 2026-10-06 02:52:14 +0800. The diff is four files, 572 insertions and 55 deletions: `contracts/gateway/parser-worker-v1.md`, `crates/oo-gateway/src/bin/oo-sql-parser-worker.rs`, `crates/oo-gateway/src/parser_worker.rs`, `crates/oo-gateway/tests/parser_worker.rs`.

This directory is an independent review of that frozen commit. Product sources, `Cargo.lock`, the ledger, and the specs were not edited. M1-D23, SP06-R8, the public gateway, and release stay open. No kernel certification campaign was run. PostgreSQL was not started.

`PARSE_DEADLINE` is 500 ms from immediately before `spawn`. The contract includes the request write, the child's read of end of file, the reply read, the group signal, the direct `wait`, and the IO join. The socket timeout is at most one 20 ms slice. The parent poll in this tree is 10 ms. The review ceiling on `ProcessReport.elapsed` is 500 + 20 + 10 = 530 ms. A 2 s watchdog fails the case if `supervise_with` is still inside the call. The decisive measurements sit between 501 ms and 509 ms on the timeout cases.

## Blobs

Git blobs at `c27d6349`:

| Path | Blob |
|---|---|
| `crates/oo-gateway/src/parser_worker.rs` | `59c1518d5d2941f04e2c51ec1585de1e9e57c283` |
| `crates/oo-gateway/src/bin/oo-sql-parser-worker.rs` | `99e04e5b9810fb8885534c685a59d49c3ad5b0d8` |
| `crates/oo-gateway/tests/parser_worker.rs` | `8b47ac63ed490a77a734218bd9ec14942b62a7f1` |
| `contracts/gateway/parser-worker-v1.md` | `14fdbcd0244c0f83c8dd640a34b53325dc2c4dbc` |
| `crates/oo-gateway/src/query_executor.rs` | `90eaf126aefd877111f773d4b2a9012ddab9103b` |
| `Cargo.lock` | `2cc7617c9e06a849de890b4ef28e5f1331988e15` |

Content SHA-256 from the decisive archive (`linux/overlay/source-sha256.txt`):

| File | SHA-256 |
|---|---|
| `parser_worker.rs` | `0021a0cf9e24bcee89e73d952c13b8a6112016e338257f805d46d78d769d3acc` |
| `oo-sql-parser-worker.rs` | `1664c540f57544185e7f155096d6d60de83b5bb4efb669d98f0d64cb1459901b` |
| `tests/parser_worker.rs` | `5e285dcce21acf8eba337b9fd1a47a0622bb6be2d22ed9306d3fb04fce52966c` |
| `parser-worker-v1.md` | `e298f3f1e2b87a7e6b613428cdbfd6fee5a3f6d4e624e94fba8bca82d7244893` |
| `query_executor.rs` | `2ad166e91a407f5bff39d814d7beb4a1bc69710e30851260528184f3e8558925` |
| `Cargo.lock` | `9608f88b7dbe46674e064f005280768cf9c383cce18827a57a8241056fa03f08` |
| review harness | `8a7243f1171525f0919ef10e7f2acfbebca020ad4ae0bc404216f94cb961b80c` |

`linux/overlay/lock-hash.txt` says the lock was unchanged at `9608f88b7dbe46674e064f005280768cf9c383cce18827a57a8241056fa03f08`. The a201 repair record quotes lock digest `1e9b9377cb2bdf951ab22f7f0a4b68649cb70e563f6ef13adad5852e1c72c657` for the tree it built from. The content hash of `Cargo.lock` at `c27d6349` is `9608f88b7dbe46674e064f005280768cf9c383cce18827a57a8241056fa03f08`.

`query_executor.rs` blob `90eaf126` differs from the copy a201 saved beside the repair. This commit does not modify `query_executor.rs`. See FINDINGS F1.

## What ran

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. No pull. `RUSTUP_TOOLCHAIN=1.94.0` overrides `rust-toolchain.toml` channel `1.98.1`. Each new builder: `--cap-drop ALL`, no-new-privileges, memory 6g (`6442450944`), `--cpus 1` (`NanoCpus=1000000000`), `HOME=/tmp`, tmpfs `/tmp` `noexec`, private network `oo-a215-eef831aa-net`, volumes `oo-a215-eef831aa-cargo` and `oo-a215-eef831aa-target`. One builder at a time. No host network, no docker socket, no host home, no shared cargo cache, no privilege. PostgreSQL image was not used.

Recipes and the edits from them are in `linux/RECIPE.txt`.

The compile tree is a `git archive` of `c27d6349`. The overlay copies only `overlay/parser_worker_a215_review.rs` onto that archive. `cmp` against `source/blobs/` ran before the container and again inside it.

## Pre-overlay smoke

`linux/preoverlay/`, before the review harness existed. Container `ec2a20b5d578cb76f3e737e2bbdf0a6b1e97dd74926eb9877a300c6fcd4d576a`. Launch duration 164.86 s. `OOMKilled=false`, `Privileged=false`, `CapDrop=["ALL"]`.

| Phase | Exit |
|---|---|
| libclang extract | 0 |
| `cargo fetch --locked --offline` on an empty volume | 101 |
| `cargo fetch --locked` online | 0 |
| `cargo test --locked --offline -p oo-gateway --test parser_worker -- --test-threads=1` | 0 |
| docker | 0 |

Author tests: `ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.56s`. `smoke.exit` is 0. `cargo update` was not run.

## Preserved harness attempts

`linux/overlay-attempt-1/` is the first overlay. Container `9e287464be3d6cfde3327c8bcc6c825c7fc375d0d2a6ab6b2e64e6549d141581`, launch duration 2.75 s, docker exit 101, not OOM. Blob compare 0, offline fetch 0, clippy component 0. The test compile failed `E0277` at `parser_worker_a215_review.rs:187`: `str::get` was called with a `usize`. `clippy.exit` is `not_run`. That file is the harness. The product archive matched the saved blobs.

`linux/overlay-attempt-2/` is the next overlay, after `text.get(from..)`. Container `bd6791bc739b04cef58136d832eef5050c7489af934c20f44fc0947518b7160d`, launch duration 95.10 s, docker exit 101, not OOM. Tests passed: `ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.26s`. Clippy failed `clippy::clone_on_copy` at line 542 on `ProcessReport`, which is `Copy`. `clippy.exit` is 101. The oracle for that run is preserved and sits inside the same 530 ms ceiling. The decisive numbers below are the later run.

## Decisive overlay

`linux/overlay/`. Container `5d5d5700621af76e815031d324f71e2759b28fe4de13bfc63020a7918f6654d5`. Launch duration 9.12 s. `ExitCode=0`, `OOMKilled=false`, `Privileged=false`, `NetworkMode=oo-a215-eef831aa-net`, `CapDrop=["ALL"]`, memory `6442450944`, `NanoCpus=1000000000`.

| Phase | Exit |
|---|---|
| blob compare | 0 |
| `cargo fetch --locked --offline` | 0 |
| `rustup component add clippy --toolchain 1.94.0` | 0 |
| `cargo test --locked --offline -p oo-gateway --test parser_worker_a215_review -- --test-threads=1 --nocapture` | 0 |
| `cargo clippy --locked --offline -p oo-gateway --test parser_worker_a215_review -- -D warnings` | 0 |
| lock unchanged | 0 |
| docker | 0 |

`test.stdout`: `ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.30s`. Clippy stderr ends `Finished dev profile [unoptimized + debuginfo] target(s) in 0.21s`. `clippy.stdout` is empty. Attempt 2 had already checked the dependency graph under clippy and stopped on the harness lint. This run rechecked `oo-gateway` after that lint was removed and exited 0.

`cargo-update-invoked.txt` is `no`.

## Oracle (decisive)

`linux/overlay/oracle/oracle.txt`. Timeout rows are `TimedOut`, signal 9, reaped, pid absent, pid reuse 0, `hung=0`, `join_returned=1`.

| Case | Wall ms | Elapsed ms |
|---|---|---|
| blocked stdin, request longer than 300000 bytes | 503 | 501 |
| hold-stdout | 506 | 503 |
| partial 4-byte reply | 508 | 507 |
| late frame after 700 ms | 503 | 502 |
| same-group grandchild, stdout inherited | 505 | 505 |
| same-group grandchild, stdout null | 505 | 503 |
| `setsid` sleep, stdout inherited | 509 | 509 |
| crash exit 99 | 2 | 2 |
| three legal plans | 2, 1, 1 | |
| identity mismatch | 1 | |
| direct trailer `0x7f` | 7 | |

The late elapsed time is under 690 ms. The crash row has an empty signal, `direct_wait=1`, and exit 99. Legal rows record `sha_match=1` and `parser_version=170007`. The identity-mismatch row records `canary_in_display=0`. The trailer row is exit 1, stdout length 0, decode `Trailing`. `--slow` and `OO_PARSER_WORKER_SLOW` both exit 2 with empty stdout.

Every timeout elapsed value is at least 500 ms and at most 509 ms. The 2 s watchdog did not fire. The join returned inside one slice plus the 10 ms poll.

## Limits of this approval

`RESTRICTION_CAPABILITY` is `Unsupported`. `UNTRUSTED_ADMISSION` is `Blocked`. The group signal does not kill a `setsid` descendant. The review measured that descendant alive, in a different process group, and then signaled only that positive pid. If `/proc` no longer names the leader, `group_is_owned` still signals the recorded group. Reuse of that id is the residual the contract states. This review did not manufacture a colliding pid. Contract line 15 describes a `query_executor.rs` sentence that this commit's file does not contain. FINDINGS F1 records that.
