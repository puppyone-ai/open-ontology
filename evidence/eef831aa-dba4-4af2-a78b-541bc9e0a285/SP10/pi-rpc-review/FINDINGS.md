# Findings

Verdict: **APPROVE_LIMITED**.

Independent reading of the sealed spike at `c551afea55d5a9164760878a5d6a76835b869875` and of this evidence directory. The parent checklist was not edited. H-D01 stays pending parent acceptance.

## Archive

The tested tree is `linux/preoverlay-msrv/spike`, a `git archive` of `testing/spikes/p09-pi-rpc` at `c551afea`. `ancestor.exit` is 0. `drive-cmp.exit` is 0 against the `b0cc1d24` `drive.rs`. `spike-diff.stdout` is empty. At report time the spike path is also identical between `c551afea` and HEAD `d738a621c296e97458e2b9dd30947dd03bc1689b`. No file under the spike was written by this review. `NO_OVERLAY`.

The source oracle requires the let chain and the 262144-byte cap, exactly one `Cargo.lock` package named `oo-p09-pi-rpc`, `rust-version` 1.94.0, and `forbid(unsafe_code)` on the four crate roots. `spike-oracle.stdout` ends `spike_ok`. `TurnFinished` is constructed only for `stop == "stop"` with no `errorMessage`. The aborted arm returns `Error`.

## Tests and clippy

Before any edit, `cargo test --locked --offline -- --test-threads=1` on Rust 1.94.0 aarch64 passed the twelve boundary tests in 0.44s. `test.exit` is 0. The oracle named the response-id pair, the out-of-order pair, clean EOF, the partial line at EOF, the deadline, and the fixture map that does not invent tool events. `boundary-oracle.stdout` ends `boundary_ok`.

Clippy was absent from the image (`components-before.txt` has no clippy line). The component was downloaded on the private bridge (`info: downloading component clippy`, `clippy-install.exit` 0) and then run offline: `cargo clippy --locked --offline --all-targets -- -D warnings`. `clippy.exit` is 0. stderr is `Checking oo-p09-pi-rpc` and `Finished dev profile` in 0.40s. Container `4dea844a67c8c59846b21df9aab518f345ceb3802d2ec493ff3aa21d2aa34742`.

a279 `linux/adapter-proof-4/clippy.exit` remains 127. The historical oracle read that file and required the digits 127. a282 `a279-compare.stdout` remains `expected=473`, `fresh=474`, `bad=0`. Neither historical tree was rewritten.

## Pin and live RPC

A fresh prefix was installed from the pinned Node v22.23.3 and Pi v1.0.4 artifacts. The Rust pin oracle matched all seven PIN digests and ended `pin_ok`. `pi --version` printed `1.0.4` with exit 0. The git checkout is `7c10bd4337495ee613f2224843ecdf349b80d1df`. The published npm package still has no LICENSE file. The git MIT text hash matches PIN. v1.0.4 stays under the seven-day floor dated 2026-10-05 in PIN.

The first download, container `94e9e7163609be234b65532f42d3f993215be19c6019944324c976cbdfef2ab1`, exited 35 on a TLS EOF while fetching the npm tarball. Node sums, the Node tarball, the git tag, the GitHub tarball, and the npm metadata had already passed. That directory is preserved. The retry, container `52faff213c0fd4d5364dbe247eaf3e46bf2d07e18f8c481268d1a10172ce091b`, exited 0.

The live container `99caa3080013c839e4e5dc9189b571af96ddcc1b365a091c91b94e2b1c5c28e5` used `--network none`. `live.stdout` is `live_ok`. `live.exit` is 0. `live-oracle.stdout` ends `live_oracle_ok`.

Cold `export` and cold `restore` are the honest unknown-command responses and match the fixtures. Abort `get_messages` contains `stopReason` `aborted` and `errorMessage` `Request was aborted`, and does not contain `"stopReason":"stop"` or `turn_finished`. The session file contains assistant text `fixture-reply` and `stopReason` `stop`. `switch_session` returns `cancelled` false. The following `get_messages` contains `stopReason` `stop` and does not contain `stopReason` `aborted`. Both mock logs record `auth_ok=true`, `local_host=true`, `sent-first`, and `127.0.0.1`, and neither log contains `public-dummy-fixture-token`.

`pi-version-pre.exit` 127 is the PATH miss (`env: 'node': No such file or directory`). The driver version check is exit 0 and stdout `1.0.4`.

## Left open

The approval is the prototype stdio spike only. The async harness trait, tool events, clarification events, artifact events, runsc, and a sandbox claim were outside this review and were not added. `--network none` is the live container setting. The plan and ledger H-D01 boxes stay unchecked.
