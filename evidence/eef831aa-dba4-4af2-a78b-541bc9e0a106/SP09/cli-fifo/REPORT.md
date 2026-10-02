# a106 CLI FIFO open report

This is the correction for `CLI-FIFO-OPEN-BLOCKS`. It is not acceptance of the `oo query` child, not SP09-W02, and not a real Gateway or PostgreSQL result. An independent review of the commit that contains this file still has to happen.

## Identity

The activation card named HEAD `647387de108a8f45c82df2d5387302f8d9f9a899`, tree `3f374c1e50609ee4b6f5d1c98b65155f2776cc6d`, parent `fcb59dd211c7b27d1e605c0ebc858d545b828a15`. That commit is an ancestor. Before the first build, the branch tip was already `dde0dff30e64c9f4c30a7b1371508de38efea553`, tree `902e30f21751d9fdf5d0c7aa63bb4bc69f8a1260`, parent `aba39fae9fe7e174cd58fa560091fecffcb39c0a`. Those two later commits are the target-review receipt and the pipeline preflight receipt. They were not rewound. The tracked worktree was clean. `Cargo.lock` blob `15ae1071c7236a956547e3f1d37d627b2cbebfce` is the same at `647387de` and at `dde0dff`. Raw sha256 `d3cf96bc32f829cc9a1e7818c064a4447b1bb5b06cffe4501e82d5617108b06f`.

Review source remains `3347f5a280da52c70525879b4608170b6b92b523`, tree `0e9f38551ea6f88c78ff980efbf2eec40c0da4ab`, parent `1b9110265aae92bdd7eb02a71dcbeacf648375a5`. At `dde0dff`, `crates/oo-cli` and `contracts/cli` matched that commit. `load.rs` blob before this change was `57024a15faea0648b95e44a0eecb5875b65c9f8a`.

This file cannot store the hash of the commit that contains it. After the commit, `git rev-parse HEAD` and `git rev-parse 'HEAD^{tree}'` are the delivery identity. The parent of that commit is `dde0dff30e64c9f4c30a7b1371508de38efea553`.

## What changed

`read_capped` opens token, SQL, and CA paths with `O_NONBLOCK | O_NOFOLLOW`, then `fstat` on that descriptor. There is no `stat` of the path before the open. A FIFO is not a regular file, so the read returns `not-file` before any byte is read and before a socket. `O_NOFOLLOW` alone does not do that: a peerless FIFO blocks in `open`. `O_NONBLOCK` does not make every device-node `open` return.

The owner probe is unchanged. It still creates a regular file with `O_NOFOLLOW` and `O_EXCL`. `unsafe_code` stays forbid. No new dependency, manifest, or lock line.

Contract text in `contracts/cli/query/README.md` says the same thing, including the device-open limit.

## Source hashes

| Path | sha256 |
| --- | --- |
| `crates/oo-cli/src/load.rs` | `b01eaf180f28e9721161fe134c969b37c1fe073e74c079d2a66f1a727d8172ff` |
| `crates/oo-cli/tests/common/mod.rs` | `e4888bdaafc35ee6bd2268f4cd6264e1fb730d18bd2c674bed9e626cf7c9b8a2` |
| `crates/oo-cli/tests/query.rs` | `c1fbe6264887f89a72e37c933577fd7cce7ddcc28d85f05d8bee79239b5a0839` |
| `contracts/cli/query/README.md` | `9bb88056ff55dcd34fb6a7bd3391e33c99fe3d0965c8324d41f9036cd6d05039` |
| `Cargo.lock` raw | `d3cf96bc32f829cc9a1e7818c064a4447b1bb5b06cffe4501e82d5617108b06f` |
| `Cargo.lock` git blob | `15ae1071c7236a956547e3f1d37d627b2cbebfce` |

`load.rs` git blob after the edit, before this evidence text was added, is `050c533978308095ab88182c70ccfa0c8f9e39db`.

## Immutable baseline and the known failure

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`. `RUSTUP_TOOLCHAIN=1.94.0`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. `cargo 1.94.0 (85eff7c80 2026-01-15)`. Fetch uses the network. This is not an air-gap claim.

Run `c1c68a79-25ad-4575-a198-dcbd21ea5952`. Container `oo-a106-msrv-c1c68a79-25ad-4575-a198-dcbd21ea5952`. Archive is `git archive` of `dde0dff` only. sha256 `179b88c4fb735c276eb09ab20c605189c2ffced7d518c3f5c7ed37efe6511236`. No worktree overlay. `/src` read-only. Private cargo and target volumes. Bridge network, not host. `HOME=/tmp`. No socket mount, no home mount, no shared cache. `--pull never`, `--init`, `--cap-drop ALL`, no capability added, `no-new-privileges`, memory 8g, cpus 4, pids 4096. `HostConfig.Tmpfs` null.

| Phase | Exit |
| --- | --- |
| rustc -vV | 0 |
| cargo -V | 0 |
| rustup component add clippy --toolchain 1.94.0 | 0 |
| cargo fetch --manifest-path Cargo.toml --locked | 0 |
| cargo test --manifest-path Cargo.toml --locked --offline -p oo-cli | 0 |
| cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-cli --all-targets -- -D warnings | 0 |
| worst | 0 |
| red probe compile | 0 |
| red probe | 0 |
| docker run | 0 |

The package test on that archive was the previous nine integration tests: 9 passed, 0 failed, 19.09s. Lib, bin, and doc tests were 0. Clippy reported no warnings. Elapsed 79s inside the container. Inspect: `ExitCode=0`, `OOMKilled=false`, `Privileged=false`, `CapDrop=["ALL"]`, `CapAdd=null`, `Memory=8589934592`, `NanoCpus=4000000000`, `PidsLimit=4096`.

The red probe exit 0 means the known product failure was reproduced. It does not mean the CLI passed. The probe spawns the archive's `oo`, sets a 1200ms deadline, then sends SIGKILL. Request `--timeout-ms` was 120000. No writer was attached. Stdout and stderr files are 0 bytes. Accepts are 0. A killed child is recorded as `product_pass=no`.

| Case | Signal | Exit code | Elapsed | Accepts | Stdout | Stderr |
| --- | --- | --- | --- | --- | --- | --- |
| `fifo_token_peerless` | 9 | none | 1218ms | 0 | empty | empty |
| `fifo_sql_peerless` | 9 | none | 1216ms | 0 | empty | empty |
| `fifo_ca_peerless` | 9 | none | 1222ms | 0 | empty | empty |

Raw files are under `baseline/out/red/`. The probe does not record argv or the environment. Summary: `reproduced=3`, `expected=3`, `product_pass=no`.

## Fixed Linux run

Run `8ed66641-38ee-47d2-89b9-26ca8b6a6606`. Same image, toolchain, and container limits. Archive of `dde0dff` plus the owned `crates/oo-cli`, `contracts/cli`, and `Cargo.lock` overlay. The lock blob matches the archive. Dirty foreign trees were not copied. Elapsed 76s.

| Phase | Exit |
| --- | --- |
| rustc -vV | 0 |
| cargo -V | 0 |
| rustup component add clippy --toolchain 1.94.0 | 0 |
| cargo fetch --manifest-path Cargo.toml --locked | 0 |
| cargo test --manifest-path Cargo.toml --locked --offline -p oo-cli | 0 |
| cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-cli --all-targets -- -D warnings | 0 |
| worst | 0 |
| docker run | 0 |

Integration tests: 11 passed, 0 failed, 19.35s. The two new tests are `fifo_opens_do_not_wait_for_a_peer` and `file_bounds_and_links_stay`. The previous nine passed again. Lib, bin, and doc tests were 0. Clippy reported no warnings. Inspect matches the baseline limits. `HostConfig.Tmpfs` null.

The new FIFO test spawns `oo` through the existing `Spec` runner. `mkfifo` is `std::process`. The harness deadline is 1200ms and `--timeout-ms` is 120000. A signal death is stored separately from the exit code. `unwrap_or(1)` on a killed child is not treated as SQL or CA exit 1. Peerless token, SQL, and CA FIFOs, and the same three paths with a writer already held, must exit with the documented `not-file` text, token exit 4 and SQL/CA exit 1, with no accept, inside that deadline. The writer is an `O_RDWR` holder opened before the child, so the test does not wait on a second open that can miss a nonblocking read.

The same run keeps the file bounds that this open path must not change. A dangling token symlink is `symlink`, exit 4. A hard link of the mode `0600` token is accepted, one hit. Token files of 4096 bytes and 4096 bytes plus one newline are accepted, one hit. 4098 bytes is `budget`, exit 4, no hit. A 65536-byte SQL file is sent, one hit. The existing test still rejects 65537 as `budget`. Directory, mode, and symlink rejections stay in `token_and_sql_files_fail_before_connect`. Valid CA, wrong host, wrong certificate, and a CA symlink stay in `protocol_transport_proxy_redirect_and_tls`. Sandbox, missing auth, and missing gateway still fail on a peerless FIFO before open, in `sandbox_and_missing_auth_do_not_read_or_connect`.

The new cargo tests were not run against the pre-fix binary. The pre-fix failure is the probe above.

## Cleanup

Both UUIDs only. For each, `container_rm=0`, `network_rm=0`, `volume_cargo_rm=0`, `volume_target_rm=0`, `work_removed=yes`. Afterwards no container, volume, or network with `oo.owner=a106` remained. No prune. a108 volumes `oo-a108-eef831aa-dba4-4af2-a78b-541bc9e0a108-cargo` and `oo-a108-eef831aa-dba4-4af2-a78b-541bc9e0a108-target` were not mounted or removed. a110's postgres container was left running. No PostgreSQL fixture was started here.

## Still not shown

Foreign uid. `chown` was not attempted under cap-drop. ACLs, a same-owner rewrite after the read, and zeroization. The owner-probe inode was not sampled. A live race that replaces the directory entry between path lookup and `fstat` was not run. The check is on the opened descriptor, and there is no pre-stat. That is not a timed race proof. Device nodes were not re-run. `O_NONBLOCK` is not a claim that every driver returns from `open`. The 16 MiB render path. Backend cancellation. Stalled stdin. The other 110 review cases were not repeated. Conversation, Gateway, PostgreSQL, sandbox `from_env` and proxy, the full SDK, supply, dual-arch, and release stay unsigned.
