# a095 CLI query report

This is the executor record for `oo query`. It is not SP09-W02 acceptance, not a real Gateway run, and not a PostgreSQL permission result. An independent review of the fixed commit still has to happen.

## Identity

Parent at commit: `1b9110265aae92bdd7eb02a71dcbeacf648375a5`

Parent tree: `94c10a1b333608f4000913c681229577de7b732b`

Parent of that commit: `77abcc1731a91cb9933847133e7ae354d289fd46`, tree `47f58c94372c293e1512f220a7fe7e87a58a03ac`, whose parent is `aeb351cfb3b8c31e305568cd655ed057d4afdbf5`.

The commit that landed between the smoke archive and this commit changes only `execution/decisions/vertical-integration-order.md`, `execution/prompts/a097-evidence-retention-followup.md`, `specs/open-ontology-development-ledger.md`, and `specs/open-ontology-v0.1-plan.md`. It does not change `Cargo.lock`, `Cargo.toml`, `crates/oo-cli`, `crates/oo-sdk`, or `crates/oo-core`.

Task baseline, ancestor of the parent: `7143b2d9a7888d242983e5b9c10ef597246d5848`, tree `244187e01e5b58463b940b33f96fe76629649898`, parent `6f7fab1e6087c2a97a28eae6b06e945e2e2b9bb6`.

SDK runtime used, unchanged since that commit (`git diff` of `crates/oo-sdk` and `crates/oo-core/src/api.rs` is empty): `bde2e8cc20a1ed951f4f7ea61ce4d9b5ab3665df`. Original SDK delivery `8efeb230afc6734a2e50eaca3f09fd78300a9c7d` remains an ancestor. This slice does not edit the SDK.

This commit cannot store its own hash. After it lands, `git rev-parse HEAD` and `git rev-parse HEAD^{tree}` are the delivery identity.

## What shipped

`oo` is a small binary. `oo query` parses flags locally, reads one explicit token source, and calls `oo_sdk::Client::query` once. The SDK performs the HTTP call and its admission retries. The CLI does not open Postgres, does not cache rows, and does not search the environment, the home directory, a keychain, or a config file for a credential.

Contract: `contracts/cli/query/README.md`.

Wire format is always `json`. `client_context` is not sent. Two stable request bodies are in `contracts/cli/query/fixtures/request-min.wire.json` and `request-limits.wire.json`.

## Capability

| Capability | This slice |
| --- | --- |
| `oo query` SQL or `-f` file | yes |
| `--json` typed response, nulls omitted, trailing newline | yes |
| Markdown table plus trusted footer | yes |
| `--auth external` with `--token-file` or `--token-stdin` | yes |
| `--auth sandbox` | rejected before any token, `OO_TOKEN`, or home read. `from_sandbox_env` is not implemented |
| `--gateway` and `--transport` required before connect | yes |
| `--max-rows` `1..=2000`, `--timeout-ms` `1..=120000` tightening only | yes |
| Exit 0 truncated success | yes |
| Exit 2 usage or non-retryable Gateway refusal | yes, same code, `class` distinguishes them |
| Exit 3 `RATE_LIMITED` or `OVERLOADED` after the SDK policy | yes |
| Exit 4 authentication or transport | yes |
| Exit 1 sandbox, local, protocol, `INTERNAL`, `AUDIT_UNAVAILABLE` | yes |
| Other commands | explicit unsupported, exit 2 |
| Export, Arrow, discovery, conversation, management, config validate, SQL validate | not implemented |
| Sandbox proxy client | not implemented |
| Real Gateway and PostgreSQL | not run |

## Dependency delta

Root `Cargo.toml` is unchanged. `Cargo.lock` adds six names to the `oo-cli` package only: `libc`, `serde`, `serde_json`, `tokio`, `rustls`, `tokio-rustls`. No registry version was added or replaced. No checksum line changed. No policy file changed. No CDLA allow was added.

Direct pins, all already locked:

| Crate | Version | Role |
| --- | --- | --- |
| libc | 0.2.189 | `O_NOFOLLOW` and `ELOOP` |
| serde | 1.0.229 | markdown scalar formatting |
| serde_json | 1.0.151 `arbitrary_precision` | request and response JSON |
| tokio | 1.53.1 `macros`, `net`, `rt-multi-thread`, `time` | one runtime around `Client::query` |
| rustls | 0.23.45 `std`, `tls12`, `aws-lc-rs` | dev-only TLS oracle |
| tokio-rustls | 0.26.5 `tls12`, `aws-lc-rs` | dev-only TLS oracle |

Dev tokio also enables `io-util` and `sync`. The `process` feature is not enabled. Tests spawn `oo` with `std::process`. `unsafe_code` stays forbidden. The owner check uses a temp probe file because `getuid` is unsafe.

Lock blobs: parent `c8715026efdfd09a36b7a5e67fa7be6757f26dbb`, this lock `887cef930ba2fdadf3541aded755dc594e5bd1d7`. Raw sha256 of the lock file `40664b41001d01db59d272e0d9fab3977842becc88998bd52eefaa0c1b7a7147`.

## Linux arm64 Rust 1.94

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`. `RUSTUP_TOOLCHAIN=1.94.0`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. Fetch uses the network. This is not an air-gap claim.

Snapshot method: `git archive` of `77abcc1731a91cb9933847133e7ae354d289fd46` plus the owned `crates/oo-cli`, `contracts/cli`, and `Cargo.lock` overlay. The later parent `1b9110265aae92bdd7eb02a71dcbeacf648375a5` adds only the four documentation paths named above, so the crate inputs of this smoke and of the commit are the same. Dirty `oo-connectors` and other foreign files were not copied. `/src` was read-only. Private cargo and target volumes. Bridge network, not host. `HOME=/tmp`. No socket mount, no home mount, no shared cache. `--pull never`, `--init`, `--cap-drop ALL`, no capability added, `no-new-privileges`, memory 8g, cpus 4, pids 4096. `HostConfig.Tmpfs` null.

Run `6fc77428-7794-4ccf-b153-1a0194fa8d82`. Container `oo-a095-msrv-6fc77428-7794-4ccf-b153-1a0194fa8d82`. Evidence `linux-194/`.

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

`query.rs`: 9 passed, 0 failed, 19.19s. Lib, bin, and doc tests: 0 tests. Clippy finished with no warnings. Inspect: `ExitCode=0`, `OOMKilled=false`, `Privileged=false`, `CapDrop=["ALL"]`, `CapAdd=null`, `Memory=8589934592`, `NanoCpus=4000000000`, `PidsLimit=4096`. Elapsed 76s inside the container.

The earlier baseline directory is the empty skeleton at `7143b2d`, run `008b065a-2b3f-4809-b679-d019e5de766c`, also exit 0. That run is not this implementation.

A macOS host `cargo test -p oo-cli` and `cargo clippy -p oo-cli --all-targets -- -D warnings` also exited 0 before this Linux run. Those commands are not the MSRV proof. Their logs were not copied into this directory.

## Tests that ran

Spawned production `oo` against loopback HTTP and rustls oracles. Expected bytes are under `contracts/cli/query/fixtures` and are independent of the SDK fixture files, which are used only as response bodies.

Covered: help and usage with zero accepts, non-UTF-8 argv, `-f -`, sandbox before a fifo read, missing auth, token mode and symlink, SQL budget and permissions, canonical wire for the minimal and limited requests, SDK fixtures plus extrema and hostile markdown, integer text `9007199254740993`, `estimated_total_rows` 424242 in JSON only, four hits on `Retry-After: 0`, one hit on `Retry-After: 9`, one hit on 401, one hit on partial and stall, protocol rejects, redirect with a second door at zero, proxy environment ignored, connect failure, production HTTP rejected before connect, custom CA, hostname mismatch, symlink CA, broken pipe without a second query. `OO_TOKEN` is set to a canary and is not the bearer. The poison home path is not created.

## Cleanup

UUID `6fc77428-7794-4ccf-b153-1a0194fa8d82` only. `container_rm=0`, `network_rm=0`, `volume_cargo_rm=0`, `volume_target_rm=0`, `work_removed=yes`. Afterwards no container, volume, or network with `oo.owner=a095` remained. No prune.

## Still pending

Full arm64 and amd64 matrix, a real Gateway, PostgreSQL permission checks, CLI conversation, a sandbox proxy, management calls, config validate, SQL validate, export, discovery, the other commands, and the rest of SP09-W02. Supply findings recorded by a084 and a089, including `SUP-CDLA-TARGET-ALL`, stay with that work. This slice did not weaken deny policy.
