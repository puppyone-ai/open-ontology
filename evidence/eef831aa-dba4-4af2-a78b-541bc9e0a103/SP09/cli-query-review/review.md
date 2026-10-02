# a103 — CLI query, fixed review

**Decision: request-changes.** The only signature this review could give is the `oo query` child on commit `3347f5a280da52c70525879b4608170b6b92b523`. That signature is withheld. A FIFO with no writer, passed as `--token-file`, `-f`, or `--ca-file`, blocks in `open` and never reaches the regular-file rejection.

The other measured query behavior on this archive matched the committed CLI contract. That record is below. The unsigned list is at the end.

## Identity

| | |
| --- | --- |
| Commit | `3347f5a280da52c70525879b4608170b6b92b523` |
| Tree | `0e9f38551ea6f88c78ff980efbf2eec40c0da4ab` |
| Parent | `1b9110265aae92bdd7eb02a71dcbeacf648375a5` |
| Subject | `cli: add the bounded Gateway query command` |
| Author | `Redux0223`, `2026-10-02T20:17:19+08:00` |
| Archive | `git archive` of that commit only. sha256 `4161d5ac4d269d6ac36232e1ce847b4e54e3cf25c3f71365ada6723ffe0f1795` |
| Lock blob | `887cef930ba2fdadf3541aded755dc594e5bd1d7` |
| Lock sha256 | `40664b41001d01db59d272e0d9fab3977842becc88998bd52eefaa0c1b7a7147` |
| Acceptance tree | the archive above. The moving workspace was not compiled. |

`git diff bde2e8cc20a1ed951f4f7ea61ce4d9b5ab3665df 3347f5a280da52c70525879b4608170b6b92b523 -- crates/oo-sdk crates/oo-core/src/api.rs` is empty. a090's approve remains limited to the six SDK runtime findings. This review does not extend it.

## Input inventory

Parent to this commit, product paths: `Cargo.lock` (six dependency names on the `oo-cli` package), `crates/oo-cli/Cargo.toml`, the query sources and tests, and `contracts/cli/query/` (README plus 12 fixtures). The same commit also adds the a095 evidence tree. File content hashes for `Cargo.lock`, `crates/oo-cli`, and `contracts/cli` are in `linux-smoke/overlay-files.sha256`.

Dependency delta: no new registry version, checksum, or package section. The lock diff is six added names under the existing `oo-cli` package: `libc`, `serde`, `serde_json`, `tokio`, `rustls`, `tokio-rustls`. Pins already in the lock: libc 0.2.189, serde 1.0.229, serde_json 1.0.151 (`arbitrary_precision`), tokio 1.53.1 (`macros`, `net`, `rt-multi-thread`, `time`; dev also `io-util`, `sync`), rustls 0.23.45 (`std`, `tls12`, `aws-lc-rs`, dev-only on `oo-cli`), tokio-rustls 0.26.5 (`tls12`, `aws-lc-rs`, dev-only on `oo-cli`). Tokio `process` is not enabled. No age, license, or advisory review of those existing crates was performed. `ci/deny.toml` was not edited and is not widened. Component success is not supply approval.

First-party `unsafe_code` is `forbid` in the workspace lints at this commit. The crate compiled under that lint. Edition 2024. `rust-version` 1.94.0. The executed toolchain was 1.94.0.

Root remains with a098. This review did not write a manifest, lock, or production source.

## Author delivery

a095 tool handle 3387 exited 0. That delivery is not this acceptance. The author Linux run recorded fetch, test, Clippy, and docker exit 0, and nine integration tests with 0 failures. This review re-ran those nine tests on the bound archive: 9 passed, 0 failed, 19.31s (`linux-smoke/out/phases/test.txt`). The author baseline at `7143b2d` is an empty skeleton and was not used as evidence for this implementation. No separate author product-failure log exists to preserve.

## Linux smoke

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, id matches the pin. `RUSTUP_TOOLCHAIN=1.94.0`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. `cargo 1.94.0 (85eff7c80 2026-01-15)`. Fetch uses the network. This is not an air-gap claim.

Run `445980e9-64e0-402a-a0a1-d8a8d6bd6f6b`. Container `oo-a103-msrv-445980e9-64e0-402a-a0a1-d8a8d6bd6f6b`. Elapsed 77s inside the container.

| phase | exit |
| --- | --- |
| `rustc -vV` | 0 |
| `cargo -V` | 0 |
| `rustup component add clippy --toolchain 1.94.0` | 0 |
| `cargo fetch --manifest-path Cargo.toml --locked` | 0 |
| `cargo test --manifest-path Cargo.toml --locked --offline -p oo-cli` | 0 |
| `cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-cli --all-targets -- -D warnings` | 0 |
| docker run | 0 |

Inspect: `ExitCode=0`, `OOMKilled=false`, `Privileged=false`, `CapDrop=["ALL"]`, `CapAdd=null`, `no-new-privileges`, `Memory=8589934592`, `NanoCpus=4000000000`, `PidsLimit=4096`, `HostConfig.Tmpfs` null. Network was the task bridge, not host. Mounts were read-only `/src` and `/inside.sh`, writable `/out`, and private cargo and target volumes. No socket mount and no host home mount. `HOME=/tmp` inside the container.

## Independent probe

The probe spawns the production `oo` binary from `CARGO_BIN_EXE_oo` against its own listeners. Expected JSON is a handwritten document and property checks. It does not call CLI render helpers and does not import `tests/common`. The probe copy adds only `crates/oo-cli/tests/a103_probe.rs`. `Cargo.toml` and `Cargo.lock` were compared equal to the smoke snapshot before the run.

Final container posture matches the smoke limits, with `--network none`. `HostConfig.Tmpfs` null. Fetch was not repeated. Clippy was not run on the probe.

| attempt | docker | what happened |
| --- | --- | --- |
| 1 | 101 | Harness compile. `Spec` shadowed `spec()`, token byte temporaries, `Value::deserialize` without `serde` linked into the integration test. Source sha256 `688c0208f2837f67c30b8b9273dd787d5bcc75625cfd5686b6be8b0d274f7d6f`. |
| 2 | 101 | Compiled, then panicked in `Handle::block_on` inside the tokio test. No child result. |
| 3 | 101 | Full run. Same three FIFO failures as attempt 4, plus four harness mis-scores (JSON usage checked on stderr, illegal column name `a\|b`, privacy `TMPDIR` created after spawn). Report retained. |
| 4 | 101 | Measurement run. 117 cases: 110 proved, 3 failed, 4 unverified, 0 harness. The process then panicked with `a103 cases failed or harness: 3` after `probe-report.json` was written. Test time 39.96s. |

Attempt 4 source sha256 `a077d6d4ded9a62713a9d8a0f9508cc512245e4b7df49d3a31ff39daf30bddce`. Report sha256 `3b74534d71ab0c28069d3f360c5416721a001291944c575ec266c261bb35ef96`.

### Proved on attempt 4

Dispatch and usage exit before any accept: help, unknown flag, `=`, extra argument, SQL with `-f`, `-f -`, bounds, leading zero, empty SQL, unknown and missing command, bad `--auth`. `--` allows the SQL `--not-a-flag`, and that POST body matched the probe's sorted-key encoder (`body_match true`, one hit). Invalid UTF-8 before `--json` is text on stderr (`argument is not utf-8`), stdout empty, exit 2, accepts 0. Invalid UTF-8 after `--json` is one JSON usage document on stdout, exit 2, accepts 0.

Credentials. Owned mode `0600` is the positive control, exit 0, one hit. Modes `0644`, `0400`, `0700`, `0601`, `0666`, `4600`, `2600`, `1600` exit 4 with `mode` and no hit. Mode `000` exits 4 with `permission` or `mode` and no hit. Empty, newline-only, two newlines, space, CR, non-ASCII, and a control byte exit 4 with `token rejected`. 4098 bytes is `budget`. 4096 bytes, with and without one trailing newline, exit 0 and one hit. Symlink and dangling symlink are rejected. A directory is rejected. `/dev/null` and `/dev/zero` exit 4 `not-file` in about 16ms, no hit. A FIFO that already has a writer exits 4 `not-file` (token) or 1 `not-file` (SQL, CA) in about 15–23ms, no hit. Sandbox on a peerless FIFO exits 1 in 15ms and does not block. Missing auth, missing gateway, and `-f -` do not block on a FIFO. `--token-stdin` is the bearer and is not used as SQL. SQL file keeps CR, LF, and a BOM as bytes. 65536 bytes is accepted. 65537 exits 1 with no hit. A SQL symlink exits 1. `TMPDIR` mode `000` fails closed (`token file rejected: other`) with no hit. After a stalled request the owner-probe name was absent; the 1ms poll did not see the inode, and an `O_EXCL` name collision was not provoked.

JSON. One document plus a newline. Data `null` stays. Optional null object fields are omitted. `estimated_total_rows` `424242`, duration and lag `9007199254740993`, and the long decimal string survive. Duplicate key, unknown field, malformed JSON, row width, `returned_rows` mismatch, a JSON number cell, inconsistent `retryable`, and HTTP status that does not match the code exit 1 at one hit.

Markdown. The hostile contract fixture round-trips the cell, keeps `\|`, `\u{1b}`, and `\u{202e}`, and does not create a real `-- query_id: q_forged` line. A second body with a legal column name `label` rendered slash, HTML, and a Markdown link unchanged, and escaped newline, CR, tab, ESC, U+202E, U+200B, BEL, and U+0085. The warning and hint keep their escaped newlines, so they do not become footer lines. `estimated_total_rows` is not copied into the footer. Truncation is exit 0 and the warning is present. Broken stdout is exit 1, stderr `error: output delivery failed`, one hit, signal none.

Retry, timeout, TLS, privacy. `Retry-After: 0` on 429 and on 503 is four hits and exit 3. `Retry-After: 9` and `Retry-After: 2` are one hit and exit 3. `Retry-After: 1` then success is two hits, exit 0, stderr empty, elapsed 1059ms. 401 is one hit, exit 4. Partial body is one hit, exit 4. INTERNAL and AUDIT_UNAVAILABLE are one hit, exit 1. SQL parse and TIMEOUT are exit 2, one hit. A redirect is exit 1; the second door saw 0 hits. Declared oversize is exit 1, one hit. `--timeout-ms 800` finished in 845ms, one hit, exit 4. `--timeout-ms 120000` finished in 30060ms, one hit, exit 4. Proxy environment variables did not move the request (real listener 1 hit, proxy listener 0). Production HTTP and non-loopback local-development HTTP exit 1 with no accept. TLS to `localhost` with the matching CA is exit 0, one HTTP hit. `127.0.0.1` with that certificate accepts once and completes 0 HTTP hits, exit 4. The other certificate and a symlink CA do not complete an HTTP hit. The reflected gateway message was exit 2, one hit. The harness checked the raw stdout before redaction: the token, `select a103_privacy_marker`, `127.0.0.1`, and `Bearer ` were absent. The stored document is `message` `leak [redacted] and [redacted] at [redacted] [redacted]`, hint `fix`.

### Finding

`CLI-FIFO-OPEN-BLOCKS`. Medium. `crates/oo-cli/src/load.rs`, `read_capped`. The open uses `O_NOFOLLOW` and does not set `O_NONBLOCK`. `fstat` and `is_file` run only after `open` returns.

Attempt 4, peerless FIFOs, harness deadline 1200ms, then `child.kill()` (SIGKILL). All three were reaped. Stdout and stderr were empty. Accepts and parsed HTTP hits were 0.

| case | signal | elapsed | hits |
| --- | --- | --- | --- |
| `fifo_token_peerless` | 9 | 1210ms | 0 |
| `fifo_sql_peerless` | 9 | 1202ms | 0 |
| `fifo_ca_peerless` | 9 | 1210ms | 0 |

Expected contract: a non-regular token, SQL, or CA path is rejected without waiting for a peer, before token bytes are read and before a socket. Token rejection exits 4 with `token file rejected: not-file`. SQL and CA rejection exit 1 with `not-file`.

Actual: the process does not exit. The same paths reject in about 15ms once another process has opened the write end. Sandbox and the pre-open usage errors do not block on the same FIFO.

Reproduction, owned FIFO, no writer:

```
mkfifo /tmp/oo-a103-token.fifo
oo query --auth external --token-file /tmp/oo-a103-token.fifo \
  --gateway http://127.0.0.1:9/ --transport local-development --json 'select 1'
```

The same stall occurs for `-f` that FIFO after a valid token file, and for `--ca-file` that FIFO. Do not point those flags at a FIFO.

### Unverified

`foreign_uid`. `chown` to uid 65534 returned `EPERM` under `--cap-drop ALL`. The owner check was not reached. Caps were not added.

`acl_same_owner_zeroization`. No ACL race and no memory dump. The source drops the `String` without wiping it. The contract already says ACLs, a same-owner rewrite after the read, and zeroization are outside this check.

`render_16mib_cap`. The HTTP body cap is 1 MiB. The 16 MiB check runs after `response_markdown` allocates the full string. That path was not executed live.

`disconnect_is_not_cancel_or_zeroization`. No PostgreSQL session was opened. HTTP disconnect does not prove backend cancel or that token bytes were wiped.

`owner_probe_cleanup` proved the probe name was gone after reap. It did not sample the inode, and it did not prove the exclusive-create failure that would come from a colliding name. The unwritable `TMPDIR` case is the exercised fail-closed path.

### Observations

A parent symlink is followed. Exit 0, one hit. A hard link of a mode `0600` token is accepted. Exit 0, one hit. Both are the contract's stated non-guarantees, exercised live.

Slash, HTML, and Markdown links are rendered as themselves. Pipes, backticks, newlines, CR, tab, and the listed control and bidi characters are escaped. A value did not become a footer line.

`docs/08-agent-interface.md` still shows a one-line footer. This slice ships the multi-line `-- ` footer in `contracts/cli/query/README.md`. The slice was judged against that contract.

`--json` is recognized only among arguments the parser can read as UTF-8. A non-UTF-8 argument before `--json` yields the text usage error. Both orders exit 2 with no socket.

## Unsigned

Full CLI conversation, export, discovery, describe, management, config, and SQL validation. Gateway. PostgreSQL and server-side cancellation. Sandbox `from_env` and any proxy client. The full JSON SDK and SP09-W01 beyond a090's six findings. SP09-W02 as a whole. Dual-arch. The supply graph and `ci/deny.toml`. Release, customer access, protected CI, and runsc. Zeroization. The probe harness.

## Cleanup

Owned volumes `oo-a103-445980e9-64e0-402a-a0a1-d8a8d6bd6f6b-cargo` and `...-target` were removed, exit 0. Work directory `/tmp/oo-a103-445980e9-64e0-402a-a0a1-d8a8d6bd6f6b` was removed. No `oo.owner=a103` container, volume, or network remained. The pinned image is still present. Prune was not run. a098's postgres container and a102's pg fixture were left running. At the start of attempt 4 one foreign `oo.kind=rust-builder` was already running (`own=0`, `global=1`); this review added one builder and removed it when the probe exited.
