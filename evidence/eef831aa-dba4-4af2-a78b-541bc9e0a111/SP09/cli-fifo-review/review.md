# a111 review — CLI-FIFO-OPEN-BLOCKS

**Decision: approve `CLI-FIFO-OPEN-BLOCKS` on commit `7acca2073ad5f7d39d24a61a2e869a8d96fe1c6c` only.**

No product change is required for this closure. `SP09-W02` stays unsigned. The `oo query` child stays unsigned. a103's 110 proved cases stay in a103 evidence. a103's four unverified cases stay unverified. This review does not sign a parent task.

## Bound input

| | |
| --- | --- |
| Session | `eef831aa-dba4-4af2-a78b-541bc9e0a111` |
| Commit | `7acca2073ad5f7d39d24a61a2e869a8d96fe1c6c` |
| Tree | `2f9f05a77ed5cd4f21fc9aed14b62dc691b074c4` |
| Parent | `dde0dff30e64c9f4c30a7b1371508de38efea553` |
| Subject | `cli: reject fifo opens without waiting for a peer` |
| Lock blob | `15ae1071c7236a956547e3f1d37d627b2cbebfce` |
| Lock sha256 | `d3cf96bc32f829cc9a1e7818c064a4447b1bb5b06cffe4501e82d5617108b06f` |
| Archive sha256 | `5a7dd8819438b1bdfe6cced72456d45a09264e67a2200e3c8c9216b848b5605d` |
| Snapshot | `git archive` of that commit. Overlay none. |
| Workspace tip left unused | `63259e0088965fa748dccc750e22f2c9603cd312` |
| Run | `3150146e-bb95-44ff-b3fe-fb84826c1d34` |

`git rev-parse` of the named commit, its tree, and its parent matched those three hashes before the documents were written. The smoke and the probe compiled that archive. The moving workspace was not the tree under test.

Owned source hashes, recomputed from the commit and again inside both containers:

| Path | sha256 |
| --- | --- |
| `crates/oo-cli/src/load.rs` | `b01eaf180f28e9721161fe134c969b37c1fe073e74c079d2a66f1a727d8172ff` |
| `crates/oo-cli/tests/common/mod.rs` | `e4888bdaafc35ee6bd2268f4cd6264e1fb730d18bd2c674bed9e626cf7c9b8a2` |
| `crates/oo-cli/tests/query.rs` | `c1fbe6264887f89a72e37c933577fd7cce7ddcc28d85f05d8bee79239b5a0839` |
| `contracts/cli/query/README.md` | `9bb88056ff55dcd34fb6a7bd3391e33c99fe3d0965c8324d41f9036cd6d05039` |
| `Cargo.lock` raw | `d3cf96bc32f829cc9a1e7818c064a4447b1bb5b06cffe4501e82d5617108b06f` |
| `crates/oo-cli/Cargo.toml` | `3dc91f91af95a4fbb61f3fb00a8220129d17d9b4658332411103972910e82a28` |

`load.rs` git blob at this commit is `050c533978308095ab88182c70ccfa0c8f9e39db`. Parent `load.rs` blob is `57024a15faea0648b95e44a0eecb5875b65c9f8a`, the same blob a103 reviewed at `3347f5a280da52c70525879b4608170b6b92b523`.

## Prior records

a103 decided request-changes on the `oo query` child because `CLI-FIFO-OPEN-BLOCKS` was open. That matrix was 117 cases, 110 proved, 3 failed, 4 unverified. Those 110 proofs stay there. This review adds the 11-test suite on commit `7acca207` and 24 independent oracle cases.

a106 baseline run `c1c68a79-25ad-4575-a198-dcbd21ea5952` is `git archive` of parent `dde0dff`. Archive sha256 `179b88c4fb735c276eb09ab20c605189c2ffced7d518c3f5c7ed37efe6511236`. The red probe exit 0 means the stall was reproduced. `product_pass=no`. Raw metas:

| Case | Signal | Exit code | Elapsed | Accepts | Stdout | Stderr |
| --- | --- | --- | --- | --- | --- | --- |
| `fifo_token_peerless` | 9 | none | 1218 ms | 0 | empty | empty |
| `fifo_sql_peerless` | 9 | none | 1216 ms | 0 | empty | empty |
| `fifo_ca_peerless` | 9 | none | 1222 ms | 0 | empty | empty |

Deadline 1200 ms. Request `--timeout-ms` 120000. `killed_child_is_pass=no`. Summary `reproduced=3 expected=3 product_pass=no`. That RED was not rerun. The parent archive hash, the parent `load.rs` blob, and those raw files bind the pre-fix tree.

a106 fixed run `8ed66641-38ee-47d2-89b9-26ca8b6a6606` archived `dde0dff` and overlaid owned files. Overlay hashes of `load.rs`, `common/mod.rs`, `query.rs`, `README.md`, and `Cargo.lock` match this commit. That run is the author's overlay record. It is not the committed-tree proof. The proof below is a fresh archive of `7acca207`.

## What the commit does

`read_capped` in `crates/oo-cli/src/load.rs` opens the path, then calls `inspect` on that descriptor. The open sets `libc::O_NOFOLLOW | libc::O_NONBLOCK` through `OpenOptions::custom_flags` before `open`. There is no `std::fs::metadata` and no `symlink_metadata` on the path. `inspect` calls `File::metadata`, rejects a non-file, then checks size, and for a token checks owner and mode `0600`, before `take(cap + 1).read_to_end`. `map_open` maps `NotFound`, `PermissionDenied`, and raw `ELOOP`. `process_owner` is unchanged: `create_new` with `O_NOFOLLOW` only, after the token path has already been accepted as a regular file. `unsafe_code` stays forbid. The lock and the CLI manifest are unchanged by this commit.

## Toolchain and capacity

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, os linux, arch arm64. `RUSTUP_TOOLCHAIN=1.94.0`. The committed `rust-toolchain.toml` pins channel `1.98.1`; the environment override is what ran. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, commit-hash `4a4ef493e3a1488c6e321570238084b38948f6db`, host `aarch64-unknown-linux-gnu`, LLVM `21.1.8`. `cargo 1.94.0 (85eff7c80 2026-01-15)`.

Limits on both containers: `--pull never`, `--init`, `--cap-drop ALL`, `CapAdd` null, `no-new-privileges`, memory and memory-swap `8589934592`, `--cpus 4`, pids 4096, `HOME=/tmp`, `CARGO_HOME=/cargo`. `/src` and the inside script read-only. `/out` read-write. Private cargo and target volumes for this run id. No docker socket, no home mount, no shared cargo cache. `HostConfig.Tmpfs` null. Labels `oo.owner=a111` and `oo.kind=rust-builder`.

Before smoke, one labeled rust-builder was running (`a107`, up 7 seconds) and `a107` postgres was up. Postgres was not counted. Global rust builders were 1, own builders 0. One slot was free. This review used that slot and did not attach to the `a107` builder. Before the probe, the smoke container was gone, global rust builders were 0, and the `a107` postgres container was still up. The probe used the free slot. Fetch was online on the smoke bridge network. Test and Clippy were `--locked --offline`. `airgap_claim=no`. The probe used `--network none` and did not fetch.

## Linux smoke

Container `oo-a111-msrv-3150146e-bb95-44ff-b3fe-fb84826c1d34`. Network `oo-a111-3150146e-bb95-44ff-b3fe-fb84826c1d34-net`. The probe source was not in this snapshot. Elapsed 76 s. Inspect: `ExitCode=0`, `OOMKilled=false`, `Privileged=false`.

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

Integration `tests/query.rs`: 11 passed, 0 failed, 19.21 s. Lib, bin, and doc tests were 0. Clippy finished in 19.19 s with zero `warning:` lines.

The eleven tests are `file_bounds_and_links_stay`, `fifo_opens_do_not_wait_for_a_peer`, `invalid_bytes_and_stdin_sql_do_not_block`, `gateway_status_retry_and_privacy`, `limits_file_stdin_and_dash_sql_match_the_wire`, `renders_sdk_fixtures_and_independent_bytes`, `sandbox_and_missing_auth_do_not_read_or_connect`, `help_and_usage_open_no_socket`, `protocol_transport_proxy_redirect_and_tls`, `token_and_sql_files_fail_before_connect`, and `broken_pipe_reports_delivery_failure_without_retry`.

`oo` binary sha256 `85aed76b84363fcfa1032aaa9a986b89d3c9ef5fcc7a616152d89652dd3a65c2`.

## Independent oracle

One attempt. No failed run was overwritten. Container `oo-a111-probe-3150146e-bb95-44ff-b3fe-fb84826c1d34`, network none, elapsed 2 s, docker exit 0. The same cargo and target volumes were reused. After the smoke, `tests/a111_probe.rs` was copied into the extracted snapshot only. Product hashes inside that tree still matched the commit. The `oo` binary hash was the same before and after the probe, and the same as the smoke binary.

Argv: `cargo test --manifest-path Cargo.toml --locked --offline -p oo-cli --test a111_probe -- --test-threads=1 --nocapture`. Result: `ok. 1 passed; 0 failed`, finished in 0.81 s. Probe source sha256 `9fb76a2611b5d2d2ce7beebab5e4de2b6f04c0b7eee5e8d3acbd52893705cdac`. Report sha256 `ac1ef1c226a175115bc591b92c66bde6a57b66adabb4a4bd1fb571bedcf70bd8`. `case_count=24`, `failed=0`, `killed_child_is_pass=false`.

The oracle is one `#[test]` that spawns the committed `oo` with `std::process`. `mkfifo` is `std::process`. A writer-held FIFO is an `OpenOptions` read-write `File` on the calling thread, dropped when the case returns. There is no writer thread. The deadline calls `child.kill` (SIGKILL). A pass requires `signal` none and `code` equal to `Some(expected)`. A killed child cannot pass, including when the expected exit is 1. Listeners are nonblocking. Request `--timeout-ms` is `120000`. Reject cases use deadline 1200 ms. Positive and TLS cases use deadline 8000 ms. Secrets come from `/dev/urandom`. Streams that contain the token, the SQL text, or `Bearer ` would be stored as `redacted` and fail the case. Private fixtures lived at `/tmp/oo-a111-private-fixtures`, directory mode `0700`, files mode `0600`, and were removed at the end of the test. They are not in this evidence directory. Private-key bodies were not copied here.

Every case had `signal` null, `secret_absent` true, and `killed_as_exit1` false. Stdout was empty on every reject. FIFO and pre-open accepts were 0.

| Case | Exit | Elapsed | Stderr bytes | Accepts | HTTP |
| --- | --- | --- | --- | --- | --- |
| `fifo_token_peerless` | 4 | 11 ms | 182 | 0 | 0 |
| `fifo_sql_peerless` | 1 | 13 ms | 183 | 0 | 0 |
| `fifo_ca_peerless` | 1 | 11 ms | 182 | 0 | 0 |
| `fifo_token_writer` | 4 | 11 ms | 182 | 0 | 0 |
| `fifo_sql_writer` | 1 | 10 ms | 183 | 0 | 0 |
| `fifo_ca_writer` | 1 | 11 ms | 182 | 0 | 0 |
| `preopen_sandbox_fifo` | 1 | 11 ms | 296 | 0 | 0 |
| `preopen_missing_auth_fifo` | 4 | 10 ms | 185 | 0 | 0 |
| `preopen_usage_gateway_fifo` | 2 | 11 ms | 175 | 0 | 0 |
| `token_4098` | 4 | 11 ms | 180 | 0 | 0 |
| `sql_65537` | 1 | 10 ms | 181 | 0 | 0 |
| `symlink_token` | 4 | 11 ms | 181 | 0 | 0 |
| `dangling_token` | 4 | 12 ms | 181 | 0 | 0 |
| `directory_token` | 4 | 10 ms | 182 | 0 | 0 |
| `directory_sql` | 1 | 11 ms | 183 | 0 | 0 |
| `positive_token_0600` | 0 | 55 ms | 0 | 1 | 1 |
| `positive_sql_file` | 0 | 56 ms | 0 | 1 | 1 |
| `token_4096` | 0 | 45 ms | 0 | 1 | 1 |
| `token_4096_nl` | 0 | 44 ms | 0 | 1 | 1 |
| `sql_65536` | 0 | 94 ms | 0 | 1 | 1 |
| `hardlink_token` | 0 | 44 ms | 0 | 1 | 1 |
| `tls_valid_ca` | 0 | 77 ms | 0 | 1 | 1 |
| `tls_wrong_host` | 4 | 37 ms | 182 | 1 | 0 |
| `tls_wrong_ca` | 4 | 47 ms | 182 | 1 | 0 |

Retained token FIFO text, peerless and writer-held:

```
error: auth token file rejected: not-file
retryable: false
retry_outcome: not_replayed
retry_policy: max_extra_attempts=3 max_wait_ms=2000 nested_cli_retries=0 attempt_count=unknown
```

SQL FIFO text uses class `local`, message `sql file rejected: not-file`, outcome `not_applicable`. CA FIFO text uses class `local`, message `ca file rejected: not-file`, outcome `not_applicable`. Sandbox text is class `sandbox`, the static `from_sandbox_env` message, exit 1, outcome `not_applicable`. Missing auth is class `auth`, message `authentication source is missing`, exit 4, outcome `not_replayed`. Missing gateway is class `usage`, message `gateway is required`, exit 2, outcome `not_applicable`. Token 4098 is `token file rejected: budget`. SQL 65537 is `sql file rejected: budget`. Symlink and dangling symlink are `token file rejected: symlink`. Directories are `not-file` for token and SQL.

Positive cases sent `POST /v1/query` with the bearer token and the SQL bytes, one accept, one completed HTTP response, empty stderr, and 238-byte stdout. That stdout is the empty markdown for `query_id: q_empty1`, `returned_rows: 0`. Hard link of the mode `0600` token is accepted. Token 4096 and 4096 plus one newline are accepted. SQL 65536 is accepted.

Valid CA completed one TLS query, exit 0. Wrong host and wrong CA exited 4 after one TCP accept and zero completed HTTP responses. Their retained stderr is:

```
error: transport transport connect failed
retryable: false
retry_outcome: not_replayed
retry_policy: max_extra_attempts=3 max_wait_ms=2000 nested_cli_retries=0 attempt_count=unknown
```

`stderr_exact` is true on those two TLS rows because `expect_stderr` was `None`, and the scorer treats a missing expectation as exact. The scorer did not compare those bytes to a pre-registered string. The bytes above are the retained files. The TCP accept on a failed handshake is a transport accept. The FIFO cases stayed at zero accepts.

## Observations

The author harness in `tests/common/mod.rs` still stores `status.code().unwrap_or(1)` at the commit lines 375 and 394. `fifo_opens_do_not_wait_for_a_peer` asserts `outcome.signal.is_none()` before it accepts the exit. The independent oracle never maps a signal to exit 1. This is recorded. It is not a required product fix for the FIFO closure.

`process_owner` still omits `O_NONBLOCK`. It runs only after `inspect` has accepted a regular token file.

Linux ignores `O_NONBLOCK` for the regular-file reads that followed. The positive cases show those reads still return the query.

Device-node `open` bounds stay in the contract text and stay unsigned. `O_NONBLOCK` does not make every driver return.

## Residuals

Unsigned, and outside this approval: foreign uid, ACL, same-owner rewrite, zeroization, arbitrary device-driver open bounds, the 16 MiB render path, backend cancellation, stalled stdin, a real conversation, Gateway, PostgreSQL, the harness as a product, the full SDK, supply, dual-arch, and release. `SP09-W02` stays unsigned. a103's four unverified items stay unverified: foreign uid `EPERM` under cap-drop, ACL / rewrite / zeroization, the 16 MiB render path, and disconnect as cancellation.

## Cleanup

Owned names for run `3150146e-bb95-44ff-b3fe-fb84826c1d34` only. `container_rm=0`, `network_rm=0`, `volume_cargo_rm=0`, `volume_target_rm=0`, `work_removed=yes`. `/tmp/oo-a111-3150146e-bb95-44ff-b3fe-fb84826c1d34` is gone. No `oo.owner=a111` container, volume, or network remained. Prune was not run. a108 volumes `oo-a108-eef831aa-dba4-4af2-a78b-541bc9e0a108-cargo` and `oo-a108-eef831aa-dba4-4af2-a78b-541bc9e0a108-target` were still present at the post-cleanup read. The `a107` postgres container was still running. No rust-builder was running. No PostgreSQL fixture was started here.

No product, manifest, lock, policy, spec, plan, ledger, or older evidence file was edited. No stage, commit, push, or release.
