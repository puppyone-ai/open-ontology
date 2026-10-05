# a187 parser worker

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a187`. Source commit `c22cf08aa5973ae2e2d95e0d2b72f98487f8555a`. Fixed ancestor `8a289f0c699abb8c707ca45ad45cb036abc3fc05` is still an ancestor of that commit.

The slice adds `oo_gateway::parser_worker`, the binary `oo-sql-parser-worker`, the integration test `tests/parser_worker.rs`, and `contracts/gateway/parser-worker-v1.md`. The parent sends one versioned frame carrying the original SQL, the exact `PolicyCatalog`, and the exact `PolicyIdentity`. The child calls the existing `guard_select` and writes one plan frame or one refusal frame. At 500 ms, and on an oversize frame, a malformed frame, a crash, or cancel, the parent sends `SIGKILL` and `wait`s.

`RESTRICTION_CAPABILITY` is `Unsupported`. `UNTRUSTED_ADMISSION` stays `Blocked`. This run does not open the public gateway.

## Builder

One private Linux arm64 container, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Toolchain `rustc 1.94.0 (4a4ef493e 2026-03-02)`, `cargo 1.94.0 (85eff7c80 2026-01-15)`. No PostgreSQL. Volumes `oo-a187-cargo` and `oo-a187-target`. Network `oo-a187-net` (bridge). `cap-drop ALL`, `no-new-privileges`, memory 8 GiB, no published ports, `/tmp` tmpfs `noexec`. `CARGO_NET_OFFLINE=true` for the compile. Clippy was installed into that container with `rustup component add clippy --toolchain 1.94.0`.

The green compile is `linux/focused-6`, container `1cb03bff63aa3f715da70b7a25ac2fad9c49aa569836fc9acbbc82cb9762b5d0`. Host config: `ExitCode=0`, `OOMKilled=false`, `Privileged=false`, `CapDrop=["ALL"]`, `PortBindings={}`.

The tree inside the container is a copy of the pre-edit archive `523951fd9bea54e38c9e6c14a2df109cf0b1b196` plus these five files:

- `crates/oo-gateway/src/parser_worker.rs`
- `crates/oo-gateway/src/lib.rs`
- `crates/oo-gateway/src/bin/oo-sql-parser-worker.rs`
- `crates/oo-gateway/tests/parser_worker.rs`
- `contracts/gateway/parser-worker-v1.md`

`Cargo.lock`, `Cargo.toml`, `crates/oo-gateway/Cargo.toml`, and `sql_guard.rs` matched the archive. The worktree copies of `query_executor.rs` and `policy.rs` differed from the archive. The container compiled the archive bytes of both. `forbidden-compare.txt` records the hashes. `Cargo.lock` before and after the run is `1e9b9377cb2bdf951ab22f7f0a4b68649cb70e563f6ef13adad5852e1c72c657`.

## Commands and exits

Pre-edit smoke, archive only, `linux/smoke`:

- `docker-run.exit` 0, `smoke.exit` 0
- phases `fetch`, `libclang`, `lib-test`, `sql-guard-test`, `clippy`, `lock-hash` all 0
- `parser_worker.rs` was absent from that archive

Focused runs, preserved:

| Directory | docker-run.exit | Result |
|---|---|---|
| `linux/focused` | 125 | runc could not create a mountpoint for a new file on the read-only archive |
| `linux/focused-2` | 101 | library compile: `str` indexing, `JoinHandle::join` ownership, renamed lifetime lint |
| `linux/focused-3` | 2 | launch refused while the worktree `query_executor.rs` differed; no container compile |
| `linux/focused-4` | 101 | `env!("CARGO_BIN_EXE_oo_sql_parser_worker")` is not the compile-time name |
| `linux/focused-5` | 101 | tests 0, Clippy 101 |
| `linux/focused-6` | 0 | tests 0, Clippy 0, lock unchanged |

Focused-6 commands, from `linux/focused-6/inside.sh`:

```
cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib --test parser_worker --test sql_guard -- --test-threads=4
cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --all-targets -- -D warnings
```

Results in `linux/focused-6/test.log` and `clippy.log`:

- library tests: 10 passed, 0 failed
- `parser_worker`: 27 passed, 0 failed, finished in 1.49s
- `sql_guard`: 24 passed, 0 failed, finished in 0.05s
- Clippy: `Finished dev profile` with exit 0

The 27 parser-worker tests include a legal `SELECT`, star expansion, count bindings, hashes, physical names, static refusals, catalog OID and option roundtrip, identity mismatch, direct binary roundtrip, empty environment on `/proc/<pid>/environ`, cancel before and after spawn, a slow child killed with signal 9 inside the deadline and absent afterwards, crash exit 99, an oversize header killed without the 30s sleep, wrong version, magic, kind, and trailer, echo rejected without the caller SQL in the error text, and a timeout that does not kill the next child.

## What was not run

No PostgreSQL container for this task. No public HTTP server. No full workspace test suite. No release, push, or parent checkbox update.
