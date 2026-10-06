# Observed Pi RPC contract

Frozen from Pi v1.0.4 commit 7c10bd4337495ee613f2224843ecdf349b80d1df after real stdio runs. This file records bytes that were seen. It does not accept H-D01.

## Transport

Pi was started as `pi --mode rpc`. One JSON object per stdout record, LF terminated. Commands are JSON objects on stdin. An optional string `id` is echoed on the matching `type: response`. Responses are not arrival-ordered with commands, so callers pair on `id`. A successful `prompt` response means the command was accepted. Model work continues as later events. `agent_settled` marked the idle point in the live runs.

Malformed JSON produced `type: response`, `command: parse`, `success: false`, and no request id. A record with no `type` produced `Unknown command: undefined`. An extra field on `get_state` was ignored and `success` was true.

stdout is the protocol. stderr was empty on the cold, abort, and restore runs below.

## Commands that ran

| Command | Result |
| --- | --- |
| `--version` | stdout `1.0.4`, empty stderr, exit 0 |
| `--help` | stdout contains `--mode`, exit 0 |
| `get_state` | `success: true`. With `--no-session` and no provider, model fields were the placeholder `unknown`. With the local fixture, model id was `fixture-echo`. |
| `get_messages` | `success: true` and `data.messages` |
| `abort` | `success: true` while idle, and `success: true` after a `text_delta`. The abort response was written after `agent_settled`. |
| `prompt` | `success: true`, `data.disposition` `started` |
| `export` | `success: false`, `error` `Unknown command: export` |
| `restore` | `success: false`, `error` `Unknown command: restore` |
| `export_html` on `--no-session` | `success: false`, `error` `Cannot export in-memory session to HTML` |
| `switch_session` | `success: true`, `data.cancelled` false, then `get_messages` returned the saved user text and assistant text `fixture-reply` |

There is no stdin command named `export` or `restore` in this build. Transcript bytes came from `get_messages` and from the session `.jsonl` Pi wrote under `--session-dir`. Restore bytes came from `switch_session` of that file, then `get_messages`. `export_html` was not retried on a file-backed session.

`switch_session` and `get_messages` were not pipelined. An earlier run that wrote both before reading lost the `switch_session` response when stdin closed. The driver sends one command and waits for its id.

## Events that ran

Observed stdout types: `response`, `agent_start`, `turn_start`, `message_start`, `message_end`, `message_update`, `turn_end`, `agent_end`, `agent_settled`.

`message_update.assistantMessageEvent.type` values seen: `text_start`, `text_delta`, `text_end`. The delta text was `fixture-reply`.

Product names used by this spike, and only these:

| Pi record | Product event |
| --- | --- |
| `turn_start` | `turn_started` |
| `message_update` `text_delta` | `assistant_delta` |
| `turn_end` with `stopReason` `stop` and no `errorMessage` | `turn_finished` |
| `turn_end` or assistant `message_end` with `errorMessage`, or `stopReason` `aborted` | `error` |

`text_start`, `text_end`, `agent_start`, `agent_end`, `agent_settled`, `message_start`, and `response` are bookkeeping. They are not product events.

`tool_call_started`, `tool_call_finished`, `clarification_request`, and `artifact_created` were not observed. This spike does not emit them. Any other `type` is unknown and rejected.

An aborted turn is an `error`. It is not `turn_finished`. The abort run still kept assistant content `fixture-reply` in `get_messages`, with `stopReason` `aborted` and `errorMessage` `Request was aborted`.

## Local fixture

The prompt runs used a Rust listener on `127.0.0.1` and the public token `public-dummy-fixture-token`. Pi sent `POST /v1/chat/completions` with `Authorization: Bearer public-dummy-fixture-token` and `stream: true`. The container network was `none`. That is not a product network-isolation claim, a sandbox claim, or a model-quality claim.

## Bounds

The driver caps a stdout line at 1 MiB, pairs by id, returns deadline, clean EOF, a partial trailing line, and a non-JSON record as failures. Shutdown closes stdin, waits, then kills and waits. The Rust stub that exercises those paths is not Pi.
