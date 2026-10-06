# Report

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a287`. Task H-D02 / SP10-W04B. This report does not accept H-D02, the parent harness, a full goal, or a release. The plan and the ledger were not edited. The spike, the root manifests, and the root lock were not edited.

Owned paths: `tools/pi-harness/**` and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a287/SP10/pi-harness/**`. Fixed integrated baseline `c551afea55d5a9164760878a5d6a76835b869875`.

The crate is a standalone workspace. It is not a root member. Registry crates are the root-lock subset at the pinned versions. Tokio 1.53.1 uses `macros`, `rt-multi-thread`, `sync`, and `time`. The child process is `std::process`.

## What the adapter does

`Harness` implements `launch`, `send`, `events`, `abort`, `export_transcript`, and `restore`. The name is `pi`. Development launch is `LocalProcessNotSandbox` with the statement `local-process-not-a-sandbox-certificate`. `RunscApproval::Absent` and `RunscApproval::Presented` return unsupported and do not spawn. That is not a gVisor certificate.

Launch requires SHA-256-bound executable and runtime paths, an existing read-only pack, an existing workspace, private home, agent, and session directories, and an explicit `http` or `https` origin. The only apiKey written is `public-proxy-marker`, and it is written only in `models.json`. The child environment is `HOME`, `PATH`, `TERM=dumb`, and `PI_CODING_AGENT_DIR`.

Commands sent are `prompt`, `abort`, `get_messages`, and `switch_session`. EOF is not success. Unknown events are incomplete, not `turn_finished`. `stopReason` `aborted` and `errorMessage` are errors, not `turn_finished`.

## Image

`docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`

`linux/arm64`, Rust 1.94.0, `--pull never`, `--cap-drop ALL`, `--security-opt no-new-privileges`, memory 6442450944, 1 CPU. PostgreSQL was not started. One builder at a time.

## Exits

| Run | docker-run.exit | What it shows | Container |
| --- | --- | --- | --- |
| `linux/preedit-boundary` | 0 | existing spike RPC tests, 12 passed, before adapter edits | `0c3895308858a2ea9a94518fcbf85cdb95cbb74eedfbeef26b2a03fcd0444f7c` |
| `linux/focused-1` | 101 | `hash_file` was `pub(crate)` and could not be re-exported | `90734ef9afe4ac57ae9cf5037565f122fa142c74f8b63b5ced35f680098dc3dc` |
| `linux/focused-2` | 101 | live digest passed `[u8; 32]` to `hex`; dead `session_dir`; collapsible `if` | `d148a43b57da3f9fdb8257c3965fea84a05b15d936b81c949306075302284098` |
| `linux/focused-3` | 101 | `ToString::to_string` is `&self`; `unwrap_err` needs `Debug` | `3af70213669b86e6b3633aac6aee5f2b2abde647dee1105a521c2058abd08dcd` |
| `linux/focused-4` | 101 | `option_env!("CARGO_BIN_EXE_...")` was empty; live nested match | `68bd4488d152af9021bd0d48e001a329abdfd15a80f3fad10d9e091c27a188ce` |
| `linux/focused-5` | 0 | 5 unit tests and 16 stub integration tests; clippy exit 0 | `0c130ea21e0efd278d56d02d44d43a946b1cf2e28cc0d082bb81bd21b029d808` |
| `linux/pi-install-1` | 0 | Pi `1.0.4`, Node `v22.23.3`, pin sha256 checks exit 0 | `12a229728cbb6ecb2c72c19d6d333820f1558fcd5bc2fab307f18d329cf206c6` |
| `linux/pi-capture-1` | 1 | probe exit 1; `capture.ndjson` has 31 lines including `agent_settled` | `da6e467c373fd84e8dc280506912ada86f4b7582362e949980edb6157e5da9d1` |
| `linux/focused-6` | 0 | 5 unit tests and 17 stub tests, including the labeled 61-tool stub; clippy exit 0 | `75a2c1b4d5c4f2b5222412d4fbbfff8a69bd94d98fef43a26445b8f9b8a597f7` |
| `linux/pi-live-1` | 0 | tool, abort, disconnect, and restore each exit 0; network `none` | `a2e82a5bd9aa9add773b6d8e1bd109153f82ecfd7c0e6eb8ee0341a7abb9c460` |

`linux/focused-5/clippy.exit` and `linux/focused-6/clippy.exit` are 0. `components-after.txt` contains `clippy-aarch64-unknown-linux-gnu`. `clippy-not-run.txt` is absent. The command was `cargo clippy --locked --offline --all-targets -- -D warnings`.

Fetch on focused-1 exited 0. The hand-written `Cargo.lock` was accepted by `cargo fetch --locked` and `cargo test --locked`.

## Live pin

Pi commit `7c10bd4337495ee613f2224843ecdf349b80d1df`. `pi --version` stdout `1.0.4`. Node `v22.23.3`.

| Pin | SHA-256 |
| --- | --- |
| npm tarball | `04910bdae661a6529e9d6869b04006f6aad01186398b04a16c6d9793667b96c5` |
| GitHub tag tarball | `a95c2ded6a2d8b56247f4205c99e3bef6241b5b51044086d4185d75c2822c521` |
| install lock | `35fd666029cb44d3cceb5a372cbacd2aecc478c69daa14898f006af291d4d869` |
| Node license | `c738ae413cf561f174e34f6961f8ca458aae2369a73640dda6234c629b98bcc4` |

The install is a private test prefix. It is not a product launch. v1.0.4 was published 2026-10-05. This run is 2026-10-06, inside the seven-day floor. The prototype runtime stays withheld from release.

`linux/pi-live-1` network mode is `none`. Pack canaries stayed `pack-canary`. Provider logs have no `Bearer ` line and no `authorization:` line. The public marker is the models.json apiKey.

Tool turn product events, in order: `turn_started`, `assistant_delta` `fixture-reply`, `tool_call_started` bash `call_fixed`, `tool_call_finished` status `ok` output `pi-harness-fixture`, `turn_started`, `assistant_delta` `fixture-reply-after-tool`, `turn_finished` stop reason `stop`. Abort ack was true, pid 70, `waited` true, `killed` false, exit 0. Disconnect ended `incomplete` reason `stdout-eof` and did not emit `turn_finished`. Restore `data.messages` matched the export.

## Hash inventory

`MANIFEST.sha256` is produced by the a204 Rust tool `sha256_manifest.rs`. `source-pi-harness.sha256` is that tool over `tools/pi-harness`. `recipe-origin.sha256` is that tool over the copied origin scripts. Host `shasum` is not the inventory.

## Cleanup

Own volumes, the private bridge, and `/tmp/oo-a287-eef831aa-dba4-4af2-a78b-541bc9e0a287` are removed after this report and recorded in `CLEANUP.txt`. `docker prune` is not run.

## Claims withheld

H-D02 stays open. The parent harness stays open. This is not a sandbox certificate, not a network claim, not an auth or WAL result, and not a model-quality claim. The 61-tool case is a stub labeled `not_upstream_real`.
