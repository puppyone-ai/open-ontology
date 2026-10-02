# a103 handoff — CLI query fixed review

**Decision: request-changes.** Do not mark the `oo query` child accepted.

## Bound input

| | |
| --- | --- |
| Session | `eef831aa-dba4-4af2-a78b-541bc9e0a103` |
| Commit | `3347f5a280da52c70525879b4608170b6b92b523` |
| Tree | `0e9f38551ea6f88c78ff980efbf2eec40c0da4ab` |
| Parent | `1b9110265aae92bdd7eb02a71dcbeacf648375a5` |
| Subject | `cli: add the bounded Gateway query command` |
| Lock sha256 | `40664b41001d01db59d272e0d9fab3977842becc88998bd52eefaa0c1b7a7147` |
| Archive sha256 | `4161d5ac4d269d6ac36232e1ce847b4e54e3cf25c3f71365ada6723ffe0f1795` |

The archive is `git archive` of that commit. The moving workspace was not the tree under test. `crates/oo-sdk` and `crates/oo-core/src/api.rs` are unchanged from `bde2e8cc20a1ed951f4f7ea61ce4d9b5ab3665df`. a090 still covers only its six SDK findings.

## Dependency delta

No new registry version. `Cargo.lock` adds six existing names on package `oo-cli`: `libc`, `serde`, `serde_json`, `tokio`, `rustls`, `tokio-rustls`. `unsafe_code` stays `forbid`. MSRV executed was Rust 1.94.0, linux/arm64. No supply approval and no `deny.toml` change.

## What was proved

Linux smoke on the bound archive: fetch 0, `oo-cli` tests 0 (9 passed, 0 failed, 19.31s), Clippy `-D warnings` 0, docker 0. Fetch used the network.

Independent probe attempt 4: 117 cases, 110 proved, 3 failed, 4 unverified. Report `probe-run/attempt-4/probe-report.json`, sha256 `3b74534d71ab0c28069d3f360c5416721a001291944c575ec266c261bb35ef96`. Source sha256 `a077d6d4ded9a62713a9d8a0f9508cc512245e4b7df49d3a31ff39daf30bddce`. Docker exit 101 is the probe panic after that report was saved.

Attempts 1 and 2 are harness failures and are kept. Attempt 3 agrees on the three FIFO failures and mis-scored four other cases; attempt 4 is the measurement.

## Finding

`CLI-FIFO-OPEN-BLOCKS`, medium, open. `read_capped` opens with `O_NOFOLLOW` and without `O_NONBLOCK`. Peerless token, SQL, and CA FIFOs did not exit within 1200ms (signal 9 from the harness, hits 0, empty stdout and stderr). A FIFO that already has a writer is rejected as `not-file`. Expected: the documented not-file error and exit, with no peer and no socket.

## Unverified

Foreign uid (`EPERM` under cap-drop). ACL, same-owner rewrite, zeroization. The 16 MiB render path. Backend cancellation. The owner-probe inode was not sampled; the name was absent after reap.

## Unsigned

Conversation and the other CLI commands. Gateway. PostgreSQL. Sandbox `from_env` and proxy. Full SDK / W02. Dual-arch. Supply and release.

## Cleanup

a103 containers, the bridge network, both cargo/target volumes, and `/tmp/oo-a103-445980e9-64e0-402a-a0a1-d8a8d6bd6f6b` are gone. Volume removal exited 0. No `oo.owner=a103` container, volume, or network remained. The pinned image is still local. Prune was not run. a098 postgres and a102 pg fixture were not touched. Root write stays with a098.

No commit, push, or PR from this review.
