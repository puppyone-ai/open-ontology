# Report

Verdict: **PROOF_PRESENT_ACCEPTANCE_PENDING**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a279`. Fixed baseline `7f500ff0a61a42ea606a281dd931099091a52ef6`. Repo HEAD when this report was written: `41d47faf5badbc72ba99c7f5b39acbde0b5fcb08`. Date: 2026-10-06.

This directory and `testing/spikes/p09-pi-rpc/` are the owned spike. H-D01 stays unchecked. The parent plan, ledger, and acceptance count were not edited. Root `Cargo.toml`, root `Cargo.lock`, and the registry subset were not edited. This session is the author record. `FINDINGS.md` is that same-session reading. It is not an independent approval.

The decisive live run is `linux/adapter-proof-4`. Container `b9880feffa0561baf10109a8240760c883f08c33849e3ffae0672f7b545de9c2`. `test.exit` 0. `clippy.exit` 127. `live.exit` 0. `docker-run.exit` 0. `live.stdout` is `live_ok`. Console line: `test=0 clippy=127 live=0`.

## What this proves

Pi v1.0.4, git commit `7c10bd4337495ee613f2224843ecdf349b80d1df`, was launched as a real process. The Rust driver `pi-rpc-live` talked to that process over stdio. A local Safe Rust mock answered `POST /v1/chat/completions` on `127.0.0.1` with the public token `public-dummy-fixture-token`. The reply text was `fixture-reply`.

| Step | Saved result |
| --- | --- |
| `pi --version` | stdout `1.0.4`, `version.exit` 0 |
| `pi --help` | `help.exit` 0, stdout contains the 6-byte flag `--mode` (10550 bytes) |
| cold `get_state` | `success: true`, model id `unknown`, `baseUrl` empty |
| cold `get_messages` | `success: true`, `messages` length 0 |
| cold `abort` | `success: true` |
| cold `export` | `success: false`, `error` `Unknown command: export` |
| cold `restore` | `success: false`, `error` `Unknown command: restore` |
| prompt, then abort after `text_delta` | `abort-response.json` `{"id":"a1","type":"response","command":"abort","success":true}` |
| abort `get_messages` | id `m1`, `success: true`, 3 messages, assistant text `fixture-reply`, `stopReason` `aborted`, `errorMessage` `Request was aborted` |
| file-backed prompt | `session.jsonl` contains assistant text `fixture-reply` and `stopReason` `stop`. The dummy token string is absent from that file |
| `switch_session` | `{"id":"sw1","type":"response","command":"switch_session","success":true,"data":{"cancelled":false}}` |
| restored `get_messages` | id `m2`, `success: true`, 3 messages, assistant text `fixture-reply`, `stopReason` `stop`, no `errorMessage` |

There is no stdin command named `export` or `restore` in this build. Transcript bytes are `get_messages` and the session `.jsonl`. Restore bytes are `switch_session` of that file, then `get_messages`. Those are the observed equivalents. The command names `export` and `restore` stay unsupported. `export_html` was not retried.

The product event map in `testing/spikes/p09-pi-rpc/CONTRACT.md` is frozen from bytes that were seen: `turn_start` to `turn_started`, `text_delta` to `assistant_delta`, a clean `turn_end` to `turn_finished`, and an aborted turn or an `errorMessage` to `error`. `tool_call_started`, `tool_call_finished`, `clarification_request`, and `artifact_created` were not observed and are not emitted.

Boundary tests in the same container: `ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.43s`. The stub process is the boundary fixture. It is not Pi.

## Pin

Recorded in `testing/spikes/p09-pi-rpc/PIN.md`. Tag `v1.0.4` is the stable GitHub release used here. It was published on 2026-10-05, so it is younger than the product seven-day artifact floor and stays in this spike.

| Item | Value |
| --- | --- |
| npm package | `@earendil-works/pi-coding-agent@1.0.4` |
| npm tarball sha256 | `04910bdae661a6529e9d6869b04006f6aad01186398b04a16c6d9793667b96c5` |
| npm dist.integrity | `sha512-+956nfMFHr5lDUVY/2Q4k+YzojzBuCaBXFgj0eSlXVGr7QVliVddKdc1Pz6yVg1dOlJQmb67doOVrlMsIcIdaw==` |
| GitHub tag tarball sha256 | `a95c2ded6a2d8b56247f4205c99e3bef6241b5b51044086d4185d75c2822c521` |
| Git MIT text sha256 | `0457f5bcec3b3b211605dfb5d1a49042fd638f3686a410fe099c24a25af13c48` (`PI-LICENSE`) |
| Install lock sha256 | `35fd666029cb44d3cceb5a372cbacd2aecc478c69daa14898f006af291d4d869` |
| Node | v22.23.3 linux-arm64, tarball sha256 `5ced2d48d1d7198739b7f86804de0171aefb6823b684b12341d3321afc3cb0b2` |
| Node license sha256 | `c738ae413cf561f174e34f6961f8ca458aae2369a73640dda6234c629b98bcc4` (`NODE-LICENSE`) |

The published npm package does not ship `LICENSE`. The git tag MIT text is the full license kept in the spike. Node and the npm tree were build-time probe inputs in a private volume. They were not installed on the host. The spike `package-lock.json` is the npm install lock, not a Cargo lock. `Cargo.lock` names only `oo-p09-pi-rpc`.

## Image and recipe

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, `--pull never`. Rust `1.94.0` (`4a4ef493e 2026-03-02`), host `aarch64-unknown-linux-gnu`, cargo `1.94.0`. Debian 13. `RUSTUP_TOOLCHAIN=1.94.0` overrides the repo `rust-toolchain.toml` channel `1.98.1`, which is not mounted into these containers.

`linux/adapter-proof-4/hostconfig.txt`: `ExitCode=0`, `OOMKilled=false`, `Privileged=false`, `NetworkMode=none`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`. Mounts: `/inside.sh` read-only, `/work` and `/proof` read-write, `/opt/pi-prefix` and `/opt/probe` read-only, private cargo and target volumes. Host home was not mounted.

Recipe origins are `linux/preedit/recipe-origin.txt`. Minimal change from the a204, a200, and a199 launch scripts: owner `a279`, 6 GiB, 1 CPU, private volumes, and this spike path.

| Origin | Path | SHA-256 |
| --- | --- | --- |
| a204 | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/overlay/launch.sh` | `bf9f17ce21783cc2cd443761b1a249f1059a3ec2989b80a4efc4437e7f775e89` |
| a204 | same tree `inside.sh` | `a93a5e00832284b805ea89236d9aace547766080316962139d3514dc58f84928` |
| a200 | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/launch.sh` | `c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541` |
| a200 | same tree `inside.sh` | `64b70ee4050267d147128abbf254be93151effcb94b67051a5c0b01f878fadf0` |
| a199 | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/overlay/launch.sh` | `1212609e380ecea5b72033fa4eb936b61edcf33430f7fe8aa84d3549017c0537` |
| a199 | same tree `inside.sh` | `452d720b6cb37131f21537c2d7adcf2f87c4f8e2241a61d9b1415a0256ad6cda` |

Pre-implementation smoke is `linux/preedit`. Container `3001e47458034c3e07680a57350d7d717fe6e9b91beac4c74b20b51cfd54f02e`. `docker-run.exit` 0. Console ends `PHASE rustc_exit=0`, `PHASE smoke_exit=0`, `worst_phase_exit=0`. That run happened before the adapter sources were the live driver.

## Clippy

`linux/adapter-proof-4/toolchain.txt` lists the installed components: `cargo`, `rust-std`, `rustc`, `rust-gdb`, `rust-gdbgui`, `rust-lldb`, `rustdoc`. Clippy is absent. `clippy.stderr` says `clippy is not installed for toolchain 1.94.0; network none forbids rustup component add`. `clippy.exit` 127. This is an honest limitation. Clippy `-D warnings` was not executed, so it is not a pass.

## Containers

Fifteen unique container ids were created. The development cap stated for this task was 12. The extra three are retries of the same driver after concrete failures. No sixteenth container was started. PostgreSQL was not started. Global `oo.kind=rust-builder` containers were none at report time. Each run used at most one builder. The Pi runtime was that same container, sequential with the build, on `--network none` after the install.

`linux/pi-live-restore-switch-2` and `linux/adapter-proof` share id `62a18417e288ed294a5bd5c902fc73e33fccf0a94cf4cbb251dfec50a2253ee8`. One process wrote the restore observation and then the first library compile. That id is counted once.

| Dir | Id | docker-run.exit | What the exits say |
| --- | --- | --- | --- |
| `preedit` | `3001e47458034c3e07680a57350d7d717fe6e9b91beac4c74b20b51cfd54f02e` | 0 | MSRV process smoke |
| `pi-install` | `cb100cbac81691e9ba50b2df75ca8c772d16795ee04607cbb2cd1f2c85d9fbc5` | 2 | `tar` chown uid 1001 under `cap-drop ALL` |
| `pi-install-2` | `843fea8f8456bfdda435499f747463cebfcd72f76a27a136545a392b509ba820` | 1 | npm package has no `LICENSE` file. Prefix still held Node and Pi for later runs |
| `pi-version` | `59057f7aaeef48d1bdc414a6a5582330e6e9b83f129ef388a239fa2e04077fe9` | 0 | `pi_version_exit=0 pi_help_exit=0` |
| `pi-rpc-cold` | `927b25d813407fb2c599f69434828df780f76e13a558f57b5fe6df1dbddead81` | 0 | cold RPC, including unknown `export` and `restore` |
| `pi-live-build` | `96a79a690d248a486fdb6cc7a949f7718d0201d0acb9980a2aa4939487856ef9` | 0 | first observe binary |
| `pi-live-abort` | `85ddfe746da2b1127eac7c3eec4736b0a79e1f53a5645572e213fee1e5910ae9` | 0 | abort predicate matched the user prompt. Content event unproven |
| `pi-live-build-2` | `1e868c82a90e165e2a7785662a3112bab7e74ec703e39ce84abfcd6706ce52f7` | 0 | rebuilt observe |
| `pi-live-abort-delta` | `38a79f46690c2d69b2adbfe25c023aebb11ab53dfea94e0e4ebfa0c7ae46c9e7` | 0 | observation of abort after `text_delta` |
| `pi-live-restore` | `bfd0b0fd321cb9d1ea23ca5d165fb7c0c49f6d9daf932b5f5850bf03734b5188` | 0 | pipelined `switch_session` lost its response |
| `pi-live-restore-switch` | `4840d6f9751b5d325d7518be68b64e00084e2e8283453e3215c5a8ed202c5b85` | 0 | observe exit 126, tmpfs `noexec` |
| `pi-live-restore-switch-2` | `62a18417e288ed294a5bd5c902fc73e33fccf0a94cf4cbb251dfec50a2253ee8` | 0 | `observe_exit=0`, then `test_exit=1` |
| `adapter-proof` | same `62a18417…` | 0 | `test.exit` 101, `clippy.exit` 1, `live.exit` 101. Wrapper exit 0 is not a test pass |
| `adapter-proof-2` | `f648fdcde2e4eacb63f8ce58685b132f4bd8579e16fbf72e4a504e8c4bcd9143` | 1 | tests 0, clippy 127, live 1, 5-byte `--mode` window |
| `adapter-proof-3` | `c56eb839d1b29eb3407e1eb1e511d5662d82de53e290cd010245be535c903862` | 1 | tests 0, clippy 127, live 1, 16-byte `cancelled` window. `switch.json` is real |
| `adapter-proof-4` | `b9880feffa0561baf10109a8240760c883f08c33849e3ffae0672f7b545de9c2` | 0 | tests 0, clippy 127, live 0 |

Failed attempt notes stay in each directory's `ATTEMPT.txt`. Those logs were not overwritten.

## Hash inventory

`tools/sha256_manifest.rs` is a byte copy of the a204 tool. Its SHA-256 is `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. It was not executed. A sixteenth container would have been required to compile it, and that container was refused. `MANIFEST.sha256` was produced by host `shasum -a 256`. `HASH-METHOD.txt` records the empty digest and the `abc` digest from that same command. `source-spike.sha256` lists the spike files.

## Cleanup

`CLEANUP.txt`. Volumes `oo-a279-eef831aa-cargo`, `oo-a279-eef831aa-target`, `oo-a279-eef831aa-npm-cache`, `oo-a279-eef831aa-pi-prefix`, and `oo-a279-eef831aa-probe-bin` were removed, each exit 0. Network `oo-a279-eef831aa-net` had 0 containers and was removed, exit 0. `/tmp/oo-a279-eef831aa-dba4-4af2-a78b-541bc9e0a279` was removed, exit 0. `docker prune` was not run. Foreign volumes and networks were left in place. The pinned Rust image was left in place.

## Claims withheld

This record does not accept H-D01. It does not claim parent acceptance, a full goal, or a release. It does not claim a full sandbox, runsc, product network isolation, a signed image, or model quality. `--network none` is the container setting for the live proof. The spike does not implement the async product `HarnessAdapter`.
