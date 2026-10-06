# Report

Verdict: **APPROVE_LIMITED**. H-D01 is not accepted.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a285`. Reviewed spike archive `c551afea55d5a9164760878a5d6a76835b869875`. That commit integrates the `b0cc1d24149f172d50cbcc7319bd7e6cc1aa1679` `drive.rs` let chain. Archive-time repo HEAD is `216e6df82a61d2bc22548ce1a5075d5f4a045648` (`linux/preoverlay-msrv/repo-head.txt`). Report-time HEAD is `d738a621c296e97458e2b9dd30947dd03bc1689b`. `git diff --name-only c551afea HEAD -- testing/spikes/p09-pi-rpc` is empty. Date: 2026-10-06.

Owned path is this directory. The spike, `PIN.md`, `CONTRACT.md`, a279, and a282 were not edited. Root `Cargo.toml`, root `Cargo.lock`, the plan, and the ledger were not edited. a207 remains the connector manifest and root-lock writer. No product overlay was applied. `NO_OVERLAY`.

There is no repository `AGENTS.md`. `docs/09-sandbox-and-agent-runtime.md` section 7 is a template. The read set was section 6 and section 6.2, ADR-009, plan P9 and SP10-W04, the spike `CONTRACT.md` and `PIN.md`, and the full a279 and a282 `REPORT.md`, `FINDINGS.md`, and `HANDOFF.md`.

## What was reviewed

The sealed `git archive` of `testing/spikes/p09-pi-rpc` at `c551afea`. `ancestor.exit` is 0. `drive-cmp.exit` is 0 against `git show b0cc1d24:testing/spikes/p09-pi-rpc/src/drive.rs`. `spike-diff.exit` is 0 and `spike-diff.stdout` is empty. The archive is the crate that was tested. The worktree was not built.

`linux/preoverlay-msrv/spike-oracle.stdout` records `drive_let_chain=present`, `abort_is_not_turn_finished_source=ok`, `cargo_lock_packages=1`, `rust_version=1.94.0`, `unsafe_forbid_lines=4`, and the three spike pin hashes for the package lock, the Pi license, and the Node license. It ends `spike_ok`.

## Image

`docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, `--pull never`. Rust `1.94.0` (`4a4ef493e 2026-03-02`). `RUSTUP_TOOLCHAIN=1.94.0`. Each new container was 6 GiB and 1 CPU. PostgreSQL was not started. One builder at a time. The 12-process figure is the concurrent development cap. This review sits outside that cap. The builder cap is 10 concurrent, with at most one for this task. It is not a lifetime container count.

Tests and the live run used `--network none`. Clippy install and the Pi/Node download used private bridge `oo-a285-eef831aa-net` (`driver=bridge`, `internal=false`). `cargo test` and `cargo clippy` stayed `--locked --offline`.

## Exits

| Dir | Container | docker-run | What the exits say |
| --- | --- | --- | --- |
| `linux/preoverlay-msrv` | `ea74ab0afd4ad22dbb638ffc84be2e5140941e8f0ad5a58c06c5e442fb7d6157` | 0 | `test.exit` 0. 12 passed, 0 failed, 0 ignored, 0.44s. `worst.exit` 0. Network none |
| `linux/preoverlay-clippy` | `4dea844a67c8c59846b21df9aab518f345ceb3802d2ec493ff3aa21d2aa34742` | 0 | `clippy-install.exit` 0. Real `clippy.exit` 0. stderr ends `Finished dev profile` in 0.40s |
| `linux/pi-reinstall` | `94e9e7163609be234b65532f42d3f993215be19c6019944324c976cbdfef2ab1` | 35 | Preserved failure. curl 35 during the npm tarball. Directory not overwritten |
| `linux/pi-reinstall-2` | `52faff213c0fd4d5364dbe247eaf3e46bf2d07e18f8c481268d1a10172ce091b` | 0 | Node v22.23.3, Pi `7c10bd4337495ee613f2224843ecdf349b80d1df`, `pi --version` `1.0.4`, `pin_ok` |
| `linux/live` | `99caa3080013c839e4e5dc9189b571af96ddcc1b365a091c91b94e2b1c5c28e5` | 0 | `live.exit` 0, stdout `live_ok`, `live_oracle_ok`. Network none |

Each hostconfig records `OOMKilled=false`, `Privileged=false`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`. Each named container was removed. `linux/pi-reinstall-2/capacity.txt` recorded `global=1` because another owner's builder was up. This task's `own` was 0, under the task cap of 1 and the global cap of 10.

## Boundary and clippy

`cargo test --locked --offline -- --test-threads=1` on the archive. The integration result is `ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.44s`. The twelve names include `pairs_response_id_and_keeps_the_event`, `pairs_out_of_order_ids`, `reports_clean_eof`, `reports_a_partial_line_at_eof`, `deadline_elapses_before_a_late_process`, and `observed_fixtures_map_without_inventing_tool_events`. `boundary-oracle.stdout` ends `boundary_ok`.

The image did not ship clippy. `components-before.txt` lists cargo, rust-std, and rustc. `clippy-install.stderr` begins `info: downloading component clippy`. `components-after.txt` includes `clippy-aarch64-unknown-linux-gnu`. The command was `cargo clippy --locked --offline --all-targets -- -D warnings`. `clippy.exit` is 0. This review did not invent a 127 for that run.

`historical-oracle.stdout` records `a279_clippy_exit=127 preserved` and `a282_a279_compare=bad=0 preserved`. Those historical files were mounted read-only and were not rewritten.

## Live pin

`linux/pi-reinstall-2/pi-git-commit.txt` is `7c10bd4337495ee613f2224843ecdf349b80d1df`. `pi-version.stdout` is `1.0.4` and `pi-version.exit` is 0. `node-version.txt` is `v22.23.3`. `pin-oracle.stdout` matches the seven PIN digests and ends `pin_ok`:

| Artifact | SHA-256 |
| --- | --- |
| Node tarball | `5ced2d48d1d7198739b7f86804de0171aefb6823b684b12341d3321afc3cb0b2` |
| Node SHASUMS256.txt | `4fe99a2ba9d552a6f51c13ed68fb11104cfa5df601aec616be689253a8139e7a` |
| npm tarball | `04910bdae661a6529e9d6869b04006f6aad01186398b04a16c6d9793667b96c5` |
| GitHub tag tarball | `a95c2ded6a2d8b56247f4205c99e3bef6241b5b51044086d4185d75c2822c521` |
| Pi LICENSE | `0457f5bcec3b3b211605dfb5d1a49042fd638f3686a410fe099c24a25af13c48` |
| Node LICENSE | `c738ae413cf561f174e34f6961f8ca458aae2369a73640dda6234c629b98bcc4` |
| package-lock | `35fd666029cb44d3cceb5a372cbacd2aecc478c69daa14898f006af291d4d869` |

`package-lock-drift.exit` is 0. `npm-ci.exit` is 0. `installed-license-absent.txt` says the published npm package has no LICENSE file. The git tag MIT text was checked and its hash matches PIN. PIN dates v1.0.4 at 2026-10-05. On this review date that pin is under the seven-day floor. It stays a prototype. It is not promoted to the root baseline. The release gate stays separate.

The first install, `linux/pi-reinstall`, stopped at `PHASE npm-tarball` with `curl: (35) TLS connect error: error:0A000126:SSL routines::unexpected eof while reading`. `docker-run.exit` is 35. `ATTEMPT.txt` points at the retry. The retry added `--retry 5 --retry-all-errors --retry-delay 2` and exited 0.

`linux/live/live/pi-version-pre.exit` is 127. stderr is `env: 'node': No such file or directory`. That pre-check invoked `pi` before Node was on `PATH`. It is the same PATH miss a282 recorded. It is not a clippy result and it is not the driver result. The driver `version.exit` is 0 and `version.stdout` is `1.0.4`. `help.exit` is 0 and the help text contains `--mode`.

| Step | Saved result |
| --- | --- |
| cold `export` | `Unknown command: export`. Oracle `cold-export=byte_match` |
| cold `restore` | `Unknown command: restore`. Oracle `cold-restore=byte_match` |
| abort response | `{"id":"a1","type":"response","command":"abort","success":true}`. Oracle `abort-response=byte_match` |
| abort `get_messages` | `stopReason` `aborted` and `errorMessage` `Request was aborted` are present. `"stopReason":"stop"` and `turn_finished` are absent |
| session file | assistant text `fixture-reply`, `stopReason` `stop`. The dummy token string is absent from the session file and from both mock logs |
| `switch_session` | `{"id":"sw1",...,"cancelled":false}`. Oracle `switch_session=byte_match` |
| restored `get_messages` | `stopReason` `stop` is present. `stopReason` `aborted` is absent |

`live-oracle.stdout` ends `live_oracle_ok`. Literal `export` and `restore` stay unsupported. Transcript bytes are `get_messages` plus the session file. Restore bytes are `switch_session` with `cancelled` false, then `get_messages`. The slow mock log records `listen=127.0.0.1`, `local_host=true`, `auth_ok=true`, `auth_other=false`, and `sent-first`. The model file uses `fixture-echo` and `http://127.0.0.1`. `--network none` on the live container is that container's network setting.

## Hash inventory

`tools/sha256_manifest.rs` is a byte copy of the a204 tool. The MSRV container compared it with the origin and printed `same_ok` for `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. The same run hashed the sealed spike (`files=25`) and the origin blobs (`files=17`). Empty and `abc` match the FIPS vectors. `tools/manifest_eq.rs` is a byte copy of the a282 compare tool (`cmp` equal). It does not hash.

`MANIFEST.sha256` in this directory is produced after this report, by the container under `linux/manifest`. `HASH-METHOD.txt` records the method. Host `shasum` was not used for this inventory.

## Cleanup

`CLEANUP.txt`. Own volumes and network `oo-a285-eef831aa-net` are removed before the inventory. `docker prune` was not run. Foreign containers were left in place. The pinned Rust image was left in place.

## Claims withheld

This record approves the prototype spike inside the frozen H-D01 boundary. It does not accept H-D01. It does not check a plan or ledger box. It does not claim parent acceptance, a full goal, or a release. It does not implement the async product `HarnessAdapter`. It does not emit tool, clarification, or artifact events. It does not claim a sandbox, runsc, or product network isolation. It does not promote Pi 1.0.4 to the root baseline.
