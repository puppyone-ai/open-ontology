# Findings

Verdict: **PROOF_PRESENT_ACCEPTANCE_PENDING**.

Same-session author reading of `testing/spikes/p09-pi-rpc` and `linux/adapter-proof-4`. This is not an independent review and it does not accept H-D01. The parent checklist was not edited.

The fresh driver run is container `b9880feffa0561baf10109a8240760c883f08c33849e3ffae0672f7b545de9c2`. `test.exit` 0 (12 passed, 0 failed, 0.43s). `live.exit` 0 (`live_ok`). `clippy.exit` 127. `docker-run.exit` 0. `OOMKilled=false`. `Privileged=false`. `NetworkMode=none`. `CapDrop=["ALL"]`. Memory `6442450944`. `NanoCpus=1000000000`.

## Launch, send, events, abort

`pi-rpc-live` checks `pi --version` equals `1.0.4` and requires `pi --help` to exit 0 with a 6-byte `--mode` window. `version.stdout` is `1.0.4` and `version.exit` is 0. `help.exit` is 0. A host byte scan of `help.stdout` found `--mode` in 10550 bytes.

Cold commands are one id at a time. `cold-export.json` is `Unknown command: export`. `cold-restore.json` is `Unknown command: restore`. `cold-get_state.json` has model id `unknown` and an empty `baseUrl`. `cold-get_messages.json` has an empty message list. `cold-abort.json` is `success: true`.

The abort path uses the slow local mock. After a `text_delta` whose text is `fixture-reply`, abort id `a1` returns `success: true`. `abort-messages.json` then has three messages. The assistant message has text `fixture-reply`, `stopReason` `aborted`, and `errorMessage` `Request was aborted`. The mapper treats that stop reason as product `error`, and it does not emit `turn_finished` for it.

## Transcript and restore

The restore path uses the fast local mock and `--session-dir`. The driver waits for the raw record type `agent_settled`, shuts the first process down, and copies the newest regular `.jsonl`. `session.jsonl` contains assistant text `fixture-reply` and `stopReason` `stop`. A search of that file found no `public-dummy-fixture-token`.

A second process, started with `--no-session`, sends `switch_session` and waits for id `sw1`. `switch.json` has `success: true` and `data.cancelled` false. `restore-messages.json` then has three messages. The assistant message has text `fixture-reply`, `stopReason` `stop`, and no `errorMessage`.

The stdin command `restore` remains an observed failure. Calling that success a `restore` command would be fabricated. The frozen equivalent is `switch_session` plus `get_messages`. `export` remains `Unknown command: export`. Transcript bytes are `get_messages` and the session file.

## Bounds covered by the stub

`tests/boundary.rs` passed these twelve cases against `pi-rpc-stub`: deadline, command encoding, kill/wait, fixture mapping, out-of-order ids, response pairing while an event is kept, line too big, partial line at EOF, clean EOF, monotonic sequences, unknown event type, unknown JSON shape. The stub is not upstream approval.

`Cargo.lock` contains one package name, `oo-p09-pi-rpc`. The crate is std-only. `unsafe_code` is forbidden. The two bins allow `clippy::redundant_closure_for_method_calls` because `.map_err(|err| err.to_string())` is required. `ToString::to_string` as a function pointer does not type-check on `Result::map_err`. That mismatch is the preserved `linux/adapter-proof` compile failure: 50 errors, `test.exit` 101, `live.exit` 101.

## Limitations that stay open

Clippy is not installed in the pinned image toolchain, and `--network none` blocks `rustup component add`. No clippy log in this tree is a clean `-D warnings` run.

`adapter-proof-2` stopped before RPC because a 5-byte window cannot match `--mode`. `adapter-proof-3` received a real `switch_session` success and then rejected it with a 16-byte window for the 17-byte text `"cancelled":false`. `get_messages` was not sent in that run. Both attempts are preserved.

`pi-live-abort` aborted on the echoed user prompt, before HTTP. Assistant content there is empty. `pi-live-restore` pipelined `switch_session` and `get_messages`; stdin EOF dropped the switch response. `pi-live-restore-switch` compiled observe onto a `noexec` tmpfs and exited 126. Later runs compile to `/usr/local/bin`.

The spike does not emit tool, clarification, or artifact events. Those records were not seen. The async product harness trait is not implemented here. v1.0.4 is younger than the seven-day artifact floor and is not promoted to the root baseline. Fifteen containers were used, three over the stated cap of 12. The extra runs are the compile retry and the two window retries.

No defect was found in the proof-4 transcript relative to `CONTRACT.md`. The open item is parent acceptance of H-D01, plus clippy on a toolchain that actually has the component.
