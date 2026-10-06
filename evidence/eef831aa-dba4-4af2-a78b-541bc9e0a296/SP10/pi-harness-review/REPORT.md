# H-D02 independent review

Verdict: **APPROVE_LIMITED_DEV_ADAPTER**. The full harness stays open. This review does not accept H-D02 in the plan, does not accept the parent harness, and does not claim a release.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a296`. Fixed archive `cc98bab7eba2c58c127435db4d2ae777797259de`, tree `a9369aab2c53afdf94616abee66b5099adc46d3e`. Owned path: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a296/SP10/pi-harness-review/**`. Product files were not edited. `tools/pi-harness` at HEAD `795b206729c212b319dfbaae85b24f740f64be39` still matches that archive. The author packet `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a287/SP10/pi-harness/` was read and was not treated as this review's proof.

The plan line for H-D02 in `specs/open-ontology-v0.1-plan.md` remains unchecked.

## What ran

Every builder used `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, Rust 1.94.0, `--pull never`, `--cap-drop ALL`, no-new-privileges, 6 GiB, 1 CPU. Own builders were removed after each run. No Postgres. No prune.

| Run | docker exit | What it shows |
| --- | --- | --- |
| `linux/preoverlay-msrv` | 1 | Author suite on the fixed archive before the overlay. `test.exit` 0 (5 unit, 17 contract). `clippy.exit` 0. Registry oracle 0 (14 triples, 373 root registry packages). Source oracle 1: the checker looked in `lib.rs` for a sentence that lives in `child.rs`. Container `c71ef70ee2d9f7d4458c0903c43ed1c3a074422396284b75104b721af30cbdbd`. |
| `linux/oracle-2` | 0 | Same archive and root lock. Corrected oracle. `source_ok`. Registry again 14 / 373. Network none. Container `26d6f52ce084d428fee2bff8e15e02887e51c134253ad2b4d67dcbeb76345d7b`. |
| `linux/overlay` | 101 | `expect_err` needs `Debug` on `Harness`. Container `655dc56d44b098270597d9d7621f9fb8031887a34bafc6753333fca368909b11`. |
| `linux/overlay-2` | 101 | Compile and clippy 0. Ten boundary tests passed. `command_response_matches_id_not_arrival` expected sequences 0,1,2 and observed 0,1,2,3. Container `899287e6d991a2af56c73d16e9d0bc71a51927ff4f13421c95b141212d9ab012`. |
| `linux/overlay-3` | 0 | `test.exit` 0, `clippy.exit` 0. Eleven boundary tests, the 5 unit tests, and the 17 contract tests. Container `18ca4ae2551a90fcbb37ffd553b89b734a1e19d3ab40f9d84ad2e85608c08bd9`. |
| `linux/pi-install` | 0 | Fresh Pi 1.0.4 and Node v22.23.3. Pin oracle `pin_ok`, published day 2026-10-05, run day 2026-10-06, age 1 day. Container `16f3f8f6033da69c3c644bbfce8a07de91c59e5350dffa5b7b414d477b9c738c`. |
| `linux/pi-live` | 1 | Four live modes exited 0. The oracle required `/sys/class/net` to be only `lo`. This host's `--network none` netns also lists kernel fallback devices. Container `c3ead30849a420a9ffba3a3bc77d43afbfb218eb80715b3c639459623438f112`. |
| `linux/pi-live-netprobe` | 0 | Route and counter snapshot of an empty `--network none` netns. Container `d8d84fe6a24e7416e91563b14f8efdeb2138defb8a272b5d71a030681358f3b8`. |
| `linux/pi-live-2` | 0 | Fresh tool, abort, disconnect, and restore, plus route tables from that container. Network mode `none`. Container `48ad79b22fd749cc5bc080d85c6448c7a645e0a702d41bc9f5ecb2c08835b348`. |
| `linux/pi-live-oracle` | 0 | `live_oracle_ok` on the pi-live-2 proof, including `hostconfig.txt`. Container `45645a4243b9f72fdd9e794ae778b5a6dff61a209066922079fb1855fe061172`. |

Repo HEAD when the author suite started is `linux/preoverlay-msrv/repo-head.txt`: `96afe70fd68a56ae8eec7504bf20e09ecd3cdf65`.

## Registry and pins

The child lock's registry triples equal `tests/fixtures/registry-triples.txt`, and each triple is present in `git show cc98bab7:Cargo.lock`. The manifest pins `tokio` `=1.53.1`, `serde` `=1.0.229`, and `serde_json` `=1.0.151`. The manifest text has no `signal-hook`, `process`, or `net` feature token. The child lock has no `signal-hook-registry`.

Pi commit `7c10bd4337495ee613f2224843ecdf349b80d1df`. npm tarball `04910bdae661a6529e9d6869b04006f6aad01186398b04a16c6d9793667b96c5`. GitHub tarball `a95c2ded6a2d8b56247f4205c99e3bef6241b5b51044086d4185d75c2822c521`. Install lock `35fd666029cb44d3cceb5a372cbacd2aecc478c69daa14898f006af291d4d869`. Git tag LICENSE `0457f5bcec3b3b211605dfb5d1a49042fd638f3686a410fe099c24a25af13c48`. Node LICENSE `c738ae413cf561f174e34f6961f8ca458aae2369a73640dda6234c629b98bcc4`. npm integrity `sha512-+956nfMFHr5lDUVY/2Q4k+YzojzBuCaBXFgj0eSlXVGr7QVliVddKdc1Pz6yVg1dOlJQmb67doOVrlMsIcIdaw==`. The published npm package has no LICENSE file (`linux/pi-install/installed-license-absent.txt`). The git tag text was checked. Age is inside the seven-day floor. The pin stays a prototype.

## Boundary overlay

`overlay/a296_boundary.rs` is the only file added to a fresh archive copy. It is not in the product tree.

`linux/overlay-3/test.stdout` records:

- `RunscApproval::Absent` and `Presented` return `ErrorKind::Unsupported` and do not create `stub-version` or `stub-rpc`.
- A finished turn is sequences 0, 1, 2. A later 400 ms read does not turn bookkeeping into an event.
- Reorder mode accepts command id `c1`. The stub writes one `turn_start` before that response, then a full turn, so the product sequences are 0, 1, 2, 3. The decoy id is not the acceptance.
- An unknown event ends as incomplete `unsupported-event`.
- A child that sleeps with `/bin/sleep` and writes no response returns `ErrorKind::Deadline` with detail `command deadline`.
- Delay mode ends as incomplete `turn-deadline`, and the spawned pid is gone within 3 seconds.
- Tool limit 61 is `ErrorKind::Bound` and does not spawn. The `tools-61` stub, labeled `not_upstream_real` in `src/bin/stub.rs`, yields 60 `ToolCallStarted` events, incomplete `tool-limit`, and a logged `"type":"abort"`.
- Event bound other than 16 is `ErrorKind::Bound` and does not spawn. Backpressure stays below 40 and does not move across the following 800 ms.
- Stdout EOF is incomplete `stdout-eof`.
- Abort mode records ack, waited, the direct child pid, and exit 0. Ignore mode records ack false, killed true, and waited true.
- Export and restore use `get_messages` and `switch_session`. The stub command log has neither `"type":"export"` nor `"type":"restore"`. The child environment keys are `HOME`, `PATH`, `PI_CODING_AGENT_DIR`, and `TERM`. `models.json` contains the public marker once.

Clippy was `cargo clippy --locked --offline --all-targets -- -D warnings`.

## Live Pi

`linux/pi-live-2` built `pi-harness-live` and `fixture-provider` from the fixed archive with `--locked --offline`, then ran tool, abort, disconnect, and restore against the private prefix. The provider listens on `127.0.0.1` and accepts only `Authorization: Bearer public-proxy-marker`. The tool command is `printf 'pi-harness-fixture\n'`.

`linux/pi-live-oracle/live-oracle.stdout` records `live_oracle_ok`. Tool events run sequence 0 through 6 and end at `TurnFinished` with stop reason `stop`, bash call id `call_fixed`, and output `pi-harness-fixture`. Tool raw contains `toolcall_end`, `tool_execution_end`, `toolUse`, and `stop`. Response commands on that raw log are `prompt` only. Abort raw events contain `fixture-reply` and do not contain `TurnFinished`. `abort/abort.txt` is `ack=true pid=62 killed=false waited=true exit=Some(0)`. Disconnect events contain `stdout-eof` and do not contain `TurnFinished`. Export and restore message arrays match. Both command fields are `get_messages`. Restore raw response commands are `prompt` and `get_messages`. Session sha256 `56012161c5194c2f881c5bde1463d4aba0e97667060f0a9449f9b3af3ad64373`. Messages sha256 `c9c9c1aead2f149f48d1c8d2ed8e1385bc397f735c5c44485033da8921f7c6b9`.

`Harness::restore` sends `switch_session` and returns an error unless that response succeeds with `cancelled` false, then it sends `get_messages`. The live restore exit is 0 and the message arrays match, so that path returned. The frozen driver does not persist the second process stdout, so `restore/raw.ndjson` has no `switch_session` line. The oracle prints `switch_session_stdout=not_captured_by_frozen_second_process`. The stub test is the command-byte record.

`/proc/net/dev` in that container shows 106 packets and 47731 bytes on `lo`, and 0 packets on every other interface. `/proc/net/route` has the header and no data rows. Every `/proc/net/ipv6_route` line names `lo`, with a zero gateway and without the gateway flag. `hostconfig.txt` says `NetworkMode=none` and `Privileged=false`. Provider logs contain `auth_ok=true` and do not contain `Bearer ` or `authorization:`. Each `models.json` has one public marker.

## Limits

Development launch still requires the statement `local-process-not-a-sandbox-certificate`. That statement is not a sandbox, network, auth, or semantic certificate. Production runsc does not spawn. The root pin was not promoted. No real provider key was used.
