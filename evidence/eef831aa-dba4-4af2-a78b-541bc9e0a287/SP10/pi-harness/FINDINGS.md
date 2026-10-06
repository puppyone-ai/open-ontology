# Findings

## F1. The fresh Pi 1.0.4 tool turn writes both message tool events and execution events

`linux/pi-capture-1/out/capture.ndjson` is 31 lines from one `pi --mode rpc` process, Pi 1.0.4, driven by the local fixture provider. The order that binds the adapter is:

- `message_update` `toolcall_start`, then `toolcall_delta`, then `toolcall_end` with `toolCall.id` `call_fixed`, `toolCall.name` `bash`, and `toolCall.arguments.command` `printf 'pi-harness-fixture\n'`
- `tool_execution_start`, two `tool_execution_update` records, then `tool_execution_end` with `isError` false and result text `pi-harness-fixture\n`
- `turn_end` `stopReason` `toolUse`, then a second `turn_start`, then `turn_end` `stopReason` `stop`

`tool_execution_update` is not the finish signal. `tool_execution_end` is. `toolcall_end` is the first record that contains the complete arguments object, so it is `tool_call_started`. `tool_execution_start` repeats the id and name and has no arguments, so it stays bookkeeping. `turn_end` `toolUse` is not `turn_finished`. The second `turn_start` is a second `turn_started`. The prompt finishes on `stopReason` `stop`.

The capture container exited 1. The probe searched `windows(14)` for `agent_settled`, which is 13 bytes. Line 31 of the same file is `agent_settled`. That failure is preserved. The detector in `src/bin/live.rs` now uses the token length. The mapping was taken from the preserved file, and the later adapter run in `linux/pi-live-1` produced the product events above.

## F2. `CARGO_BIN_EXE_<name>` was empty in the integration test binary

`linux/focused-4` compiled the contract tests and then every launch test panicked in `option_env!("CARGO_BIN_EXE_pi_harness_stub")`. The stub tests now take the executable from `std::env::current_exe` two directories up, which is the cargo target directory, not `/tmp`. The builder tmpfs is `noexec`, so the version-mismatch script is written beside that stub binary.

## F3. Hand-written lock subset was accepted

`linux/focused-1/fetch.exit` is 0. `cargo test --locked` and `cargo clippy --locked` later exited 0 on that same lock. No `cargo update` and no `cargo generate-lockfile` were run. Tokio features do not include `process`, `signal`, or `net`.

## F4. Abort ack can reap without `kill`

`linux/pi-live-1/abort/abort.txt` records `ack=true pid=70 killed=false waited=true exit=Some(0)`. Pi exited after the abort response. The adapter did not call `kill`. The pid is the spawned child. This is not an escaped-containment claim.

## F5. Disconnect is stdout EOF, not `turn_finished`

`linux/pi-live-1/disconnect/events.json` ends `Terminated(Incomplete { reason: "stdout-eof" })`. There is no `TurnFinished`. The provider log for that run has `auth_ok=true` and does not contain the header value.

## F6. Development launch is not a sandbox certificate

`RunscApproval` does not spawn. The development flag string is `local-process-not-a-sandbox-certificate`. Container `network=none` on the live run is the docker setting. The 61-tool abort is `not_upstream_real_sixty_one_tools_abort` against the stub, and it passed in `linux/focused-6`.

## F7. Pin is inside the seven-day floor

v1.0.4 was published 2026-10-05. The install and live runs are 2026-10-06. The private prefix is not promoted. No root manifest was edited. a207 remains the connector manifest writer.
