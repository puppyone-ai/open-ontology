# Pi harness adapter contract

Frozen before the adapter implementation. This file is the H-D02 / SP10-W04B protocol for `tools/pi-harness`. It does not accept the parent harness, H-D01, or a release.

The read set before this freeze was `docs/09-sandbox-and-agent-runtime.md` section 6, `specs/open-ontology-v0.1-spec.md` REQ-RT-04, plan SP10-W04, the full a279 and a282 `REPORT.md` / `FINDINGS.md` / `HANDOFF.md`, `testing/spikes/p09-pi-rpc/CONTRACT.md` and `PIN.md`, and the pinned v1.0.4 documents saved by that spike at `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a279/SP10/pi-rpc-probe/linux/pi-install-2/upstream-rpc.md`, `upstream-rpc-commands.md`, and `upstream-json.md`. Those documents came from git commit `7c10bd4337495ee613f2224843ecdf349b80d1df`.

## Pin

Pi v1.0.4, commit `7c10bd4337495ee613f2224843ecdf349b80d1df`, Node v22.23.3 linux-arm64. The checksums are the spike `PIN.md` values. v1.0.4 was published on 2026-10-05. That is inside the seven-day artifact floor. The runtime stays a prototype. This crate does not promote it, and it does not install it. A test container may reuse the pinned download. Product launch receives an already-present executable and runtime path.

## What this crate is

An async Rust adapter over the pinned Pi RPC stdio process. Direct registry dependencies are tokio 1.53.1, serde 1.0.229, and serde_json 1.0.151, each an exact root pin. Every registry package in this crate's lock is a root-lock subset. No new crate, no `cargo update`, and no lock regeneration that moves a version. Tokio's `process` feature is not enabled: the root lock has no `signal-hook-registry`, and that feature would be a new dependency. The child is `std::process`. Tokio supplies the runtime, the bounded event channel, and the deadlines.

`unsafe` is forbidden. The crate is not a member of the root workspace.

## Launch

`launch` is the only way to start a process. It requires all of the following. Missing or ambient values fail. The adapter does not read `HOME`, does not read an API key from the environment, and does not install Node or Pi.

| Input | Rule |
| --- | --- |
| Executable | Explicit path. SHA-256 of the file bytes must equal the supplied digest before spawn. |
| Runtime | Explicit Node binary path. SHA-256 must match. Its directory is the only entry placed on `PATH`. |
| Pack | Explicit directory. The adapter never writes under it, and it cannot be the workspace, home, agent directory, or session directory. |
| Workspace | Explicit cwd for the child. |
| Session paths | Explicit private home, agent directory, and session directory. `HOME` and `PI_CODING_AGENT_DIR` are set to those paths. |
| Proxy origin | Explicit absolute `http` or `https` origin with no userinfo, query, or fragment. Written as the model `baseUrl`. |
| Model | Explicit provider id and model id. |
| Runtime mode | `DevelopmentLocalProcess(LocalProcessNotSandbox)` or `ProductionRunsc(RunscApproval)`. |
| Raw event log | Optional explicit path. When set, the adapter appends stdout records, capped at 8 MiB. Thinking prose is replaced before that write. The path is not read from the environment. |

`LocalProcessNotSandbox` is constructed only by `acknowledge()`. Its statement is `local-process-not-a-sandbox-certificate`. A development launch is a local process. It is not a sandbox certificate, not runsc, and not a gVisor claim.

`RunscApproval::Absent` and `RunscApproval::Presented` both return `unsupported` and do not spawn. This unit has no runsc verifier and does not start gVisor. A presented digest is not treated as approval to launch.

The child environment is cleared. The only variables set are `HOME`, `PATH`, `TERM=dumb`, and `PI_CODING_AGENT_DIR`. The public proxy marker `public-proxy-marker` is written solely as the Pi `apiKey` field in the agent `models.json`, because this Pi build requires that field. It is not a provider key and not a session bearer. It is not placed in the environment or in argv. The adapter has no parameter that accepts a real key.

RPC argv is `pi --mode rpc` plus the session flag, `--provider`, `--model`, and `--no-extensions --no-mcp --no-skills --no-prompt-templates --no-context-files`. A new turn uses `--session-dir`. Restore uses `--no-session` and then `switch_session`. `--no-tools` is not passed: the pinned harness tools stay available. Launch checks `pi --version` equals `1.0.4` before the RPC process.

## Commands

Stdin and stdout are LF-delimited JSON objects. A line is at most 1 MiB including the LF. A longer line is `too-big` and is not a success. A command carries a unique string `id`. The matching `type: response` repeats that id. Responses are not waited on by arrival order. One command is outstanding. Events that arrive while it is outstanding stay in the event stream. They are not dropped and they are not treated as the command result.

EOF, a partial trailing line, a deadline, or a process exit while a command is outstanding is a failure. It is not a successful response.

The adapter sends only `prompt`, `abort`, `get_messages`, and `switch_session`.

`export` and `restore` are not sent. a279 and a282 observed `Unknown command: export` and `Unknown command: restore`. Transcript bytes are `get_messages` plus the regular session `.jsonl`. Restore bytes are `switch_session` of that exact path, then `get_messages`, in a fresh process.

A successful `prompt` response means acceptance. `data.disposition` is returned to the caller. Acceptance is not `turn_finished`.

## Product events

Sequence numbers start at 0 and increase by 1 for each product event. Bookkeeping records do not consume a sequence number.

Mapped from bytes a279 and a282 actually observed:

| Upstream record | Product event |
| --- | --- |
| `turn_start` | `turn_started` |
| `message_update` with `assistantMessageEvent.type` `text_delta` | `assistant_delta` carrying that `delta` |
| `turn_end` whose message has `stopReason` `stop` and no `errorMessage` | `turn_finished` |
| `turn_end` whose message has `errorMessage`, or `stopReason` `aborted` | `error` |
| assistant `message_end` with `errorMessage` | `error` |
| `message_update` `toolcall_end` | `tool_call_started` |
| `tool_execution_end` | `tool_call_finished` |

Ignored bookkeeping, already observed: `text_start`, `text_end`, `agent_start`, `agent_end`, `agent_settled`, `message_start`, `response`, `message_update` `toolcall_start`, `message_update` `toolcall_delta`, `tool_execution_start`, `tool_execution_update`, and `turn_end` whose `stopReason` is `toolUse` and which has no `errorMessage`.

`turn_finished` carries the observed `stopReason`. Usage and answer text are copied only from fields present on that record. They are not invented. A `toolUse` turn end is not `turn_finished`. The later `turn_start` is another `turn_started`. The prompt is finished only by `stopReason` `stop`.

### Reasoning lifecycle

Pi 1.0.4 reasoning records are legitimate bookkeeping. They are not unknown, and they are not `turn_finished`. The shapes were read from the installed tree on volume `oo-a406-eef831aa-pi-kit` at `/opt/pi-runtime-kit`, producer commit `9f8d9064e8287ddb0629df6cc62940b880e48492`. `pi-coding-agent`, `pi-ai`, and `pi-agent-core` are all 1.0.4.

`dist/modes/json-event.js` function `toJsonEvent` writes each `message_update` as exactly `{type, usage, assistantMessageEvent}` and strips `partial`. After that strip, `pi-ai` `AssistantMessageEvent` gives:

| `assistantMessageEvent.type` | Exact keys besides `type` | Adapter result |
| --- | --- | --- |
| `thinking_start` | `contentIndex` number | ignored, no sequence number |
| `thinking_delta` | `contentIndex` number, `delta` string | ignored, no sequence number |
| `thinking_end` | `contentIndex` number, `content` string | ignored, no sequence number |
| `error` | `reason` `aborted` or `error`, `error` object | `error`, then incomplete |

`Usage` on those new records is `input`, `output`, `cacheRead`, `cacheWrite`, `totalTokens`, and `cost` with `input`, `output`, `cacheRead`, `cacheWrite`, and `total`. Optional `reasoning` and `cacheWrite1h` are numbers. `reasoning` is a token count, a subset of `output`, not prose. `pi-ai/dist/api/openai-completions.js` `parseChunkUsage` builds that object. Existing `text_delta` and tool records do not grow a usage requirement.

A missing key, an extra key, or a non-integer `contentIndex` on a thinking or assistant-error event is `unsupported-event:message_update:<type>`. A non-string `delta` or `content`, or a usage object that is not the shape above, is a shape failure. Neither path emits `turn_finished`.

The optional raw log stores `thinking_delta.delta`, `thinking_end.content`, a `thinking` content block's `thinking` string, and a string `usage.reasoning` as `redacted`. An unparsed line that names `thinking_start`, `thinking_delta`, `thinking_end`, or a `"thinking"` field is stored as `{"type":"redacted","reason":"unparsed-thinking"}`. A line that parses and contains no thinking prose stays byte-identical. Product events do not carry reasoning prose. `models.json` keeps `"reasoning": false`.

Assistant-stream `error` is fail-closed. The product reason is `assistant-stream:error` or `assistant-stream:aborted`. The nested assistant message and its `errorMessage` are not copied.

`start`, `done`, `thinking_partial`, and `thinking_level_changed` stay unknown. Any other type stays `incomplete` with `unsupported-event` and must not emit `turn_finished` afterward. This is not a permissive fallback. Truncation, disconnect, provider `errorMessage`, abort, the tool limit, and stdout backpressure stay fail-closed.

Installed file SHA-256:

| File | SHA-256 |
| --- | --- |
| `pi-coding-agent/dist/modes/json-event.js` | `5f3756470739fa4a1595feeaf1ce2dfd29e36a2cffd9e642f914fc23f540d606` |
| `pi-coding-agent/dist/modes/json-event.d.ts` | `8ed959fe14cf0ce76f47309cf0a503d3e21a0ab86793b22e5759dd4d6f747e65` |
| `pi-coding-agent/docs/json.md` | `839a2a2da8112298a7f8870afa71d1998db2d43b8f68c9c179a0c696b6477a5e` |
| `pi-coding-agent/docs/message-types.md` | `c642db2a93e6433aadde1675f51e3c8b51520d9a6ee1ac805802e635aab10413` |
| `pi-coding-agent/dist/index.d.ts` | `b254e36846b1dcc64ce1a8ba72e23fb410df4aa4408ba8c23e69e5b3f934e3cc` |
| `pi-coding-agent/package.json` | `248decde30021109984def7ddc8f7bf4caf21ef04910a1303ab19604206c1bd5` |
| `pi-ai/dist/types.d.ts` | `2c7f376a17195980ec4795d7ed0efd03bee4ff049b998dff7b26510d872d02b0` |
| `pi-ai/dist/api/openai-completions.js` | `5a79cc5aa41dee180692695e65771720d1ea556e9a978649dc2040278be51753` |
| `pi-ai/package.json` | `c4e921309a8d3a91ffce9bef0bd151463ad31f0f151a8534e39ae3528cc27834` |
| `pi-agent-core/package.json` | `3aecf0b11ec6d69846d104c96fa0b529bafb72ee8ab6d4c289a5d86ae309ace8` |

This section does not accept H-D02, the parent harness, or a release.

### Tool events from the fresh Pi 1.0.4 capture

The raw records are `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a287/SP10/pi-harness/linux/pi-capture-1/out/capture.ndjson`. The probe process exited 1 because it looked for a 14-byte `agent_settled` window; the token is 13 bytes and the file contains it. The mapping uses those bytes, not the upstream document.

| Observed record | Product event | Fields copied |
| --- | --- | --- |
| `assistantMessageEvent.type` `toolcall_end` | `tool_call_started` | `toolCall.name`, `toolCall.id`, compact JSON of `toolCall.arguments` |
| `tool_execution_end` | `tool_call_finished` | `toolCallId`; status `ok` when `isError` is false and `error` when `isError` is true; text blocks in `result.content` |

`toolcall_start` names the tool before the arguments object exists. `toolcall_delta` is a partial argument string. `tool_execution_start` repeats the id and name and has no arguments. `tool_execution_update` is not the finish signal: the same run wrote `tool_execution_end` with `isError` and `result`. Those four stay bookkeeping. Argument and output summaries are at most 512 bytes; a longer value keeps the first 512 bytes and `...`.

`clarification_request` and `artifact_created` are not part of this unit. They were not observed. The parent harness item stays open.

Any other `type`, or any other `assistantMessageEvent.type`, is an unknown event. The recognized thinking records and the assistant-stream `error` record above are the only additions. The turn becomes `incomplete` with reason `unsupported-event`. The stream must not emit `turn_finished` afterward.

`aborted`, a provider or message `errorMessage`, stdout EOF, and process disconnect are `error` or `incomplete`. They are not `turn_finished`.

## Bounds

| Bound | Value |
| --- | --- |
| Request and event line | 1 MiB |
| Event channel | 16 records, then the reader stops taking stdout |
| Turn deadline | caller-supplied, enforced by the adapter after a prompt is accepted |
| Command, abort-ack, and kill-wait deadlines | caller-supplied |
| Tool executions | 60. The 61st starts an abort path and ends `incomplete` with `tool-limit`. It does not emit `turn_finished`. |

A test that injects 61 tool records through a stub is labeled `not_upstream_real`. It is not a Pi run.

Stdout backpressure is the bounded channel: a consumer that stops receiving causes the reader to stop reading. Pi's own document says a client that stops reading can stall the process. This adapter relies on that and does not claim a stronger sandbox.

## Abort and reap

`abort` sends `abort`, waits for the matching success response until the ack deadline, closes stdin, waits until the kill-wait deadline, then kills and waits if the child is still alive. The returned pid is `Child::id` of the process this adapter spawned. The reap record says whether `wait` was observed and whether this adapter called `kill`. It does not claim the process could not escape, and it is not a containment certificate.

## Transcript and restore

`export_transcript` sends `get_messages` and reads one regular `.jsonl` file under the session directory. Symlinks are refused. More than one `.jsonl` is ambiguous and fails. The file must be within the transcript byte bound. The version tuple is Pi `1.0.4`, the session path, the SHA-256 of the `get_messages` response bytes, and the SHA-256 and length of the file. No `export` command is sent.

`restore` checks that same path and file hash, spawns a fresh process, sends `switch_session` with that exact path, requires `success: true` and `data.cancelled: false`, then sends `get_messages` and returns those response bytes. A hash mismatch does not switch. No `restore` command is sent. EOF during either command is a failure.

## Integration that must be a real Pi process

The live proof uses a fresh private container, the pinned Pi and Node, and a local Safe Rust provider on `127.0.0.1`. The provider accepts only `Authorization: Bearer public-proxy-marker`. Container network is `none`. That is the container setting, not a product network or sandbox claim.

The provider script is: one text delta, then one tool call whose command is a fixed `printf` of a constant string, then a final text delta and `stop` after Pi returns the tool result. The `printf` runs in that container and does not name a host path. The same proof aborts a second turn after the first text delta, records stdout EOF or process death as incomplete, and restores the finished session so `get_messages` matches the exported messages.

No customer key, provider key, session bearer, host home, full gVisor, auth store, or model-quality claim is in scope.
