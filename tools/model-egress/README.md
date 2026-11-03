# Model egress (lane A)

Python 3.10+ standard library; no install step. Run from the repository root:

```sh
python3 tools/model-egress/model_egress.py --config stack.yaml --meter usage.jsonl
python3 tools/model-egress/model_egress.py --config stack.yaml --meter usage.jsonl --probe
python3 -m unittest discover -s tools/model-egress/tests -v
```

The config is JSON (see oo-stack), including `model.provider`, `base_url`,
`model`, `key: {"secret_ref":"model-key"}` and optional `context_window`.
Default listen is **127.0.0.1:8317**, 30 requests/minute, serial calls, no retries.
`--listen` accepts literal loopback only (including ::1); `--port 0` selects
an ephemeral port for tests. Never expose this owner-local port via forwarding.
Other local processes share this trust boundary; this is not multi-user auth.
Browser Origin and foreign Host headers are rejected. Environment HTTP proxy
settings are not used. TLS uses system validation; redirects are refused.
The server accepts POST `/v1/chat/completions`; it does not expose upstream keys.

Key bytes come only from the named 0600 owner file under the 0700 secret store.
Parent aliases are canonicalized; directory-fd traversal refuses private-leaf symlinks. Keys never enter argv, logs or meter
records. Access/error logging is disabled; errors use fixed codes. The JSONL
meter includes duration, stream flag, outcome and measured token counts (null
when unavailable), never prompt, response, credential, endpoint or headers.
The meter is owner-only, no symlinks/hardlinks. Streaming metering completes
on terminal event, disconnect or failure, not on receipt of headers.

OpenAI requests retain provider-compatible fields and stream usage is requested.
Anthropic translation supports text user/assistant/system messages, output
limit, temperature/top_p/stop, non-stream responses and text SSE. Tools, images,
structured-output translation and other unsupported Anthropic fields fail
explicitly rather than silently dropping semantics. Upstream 429, quota and
cooldown refusals become RATE_LIMITED, USAGE_LIMIT and COOLDOWN with an immediate
local cooldown (60s; quota 300s). Authentication and other errors are sanitized.
Streams require `[DONE]` (OpenAI) or message_stop (Anthropic); clean EOF without
that terminal marker is still STREAM_INCOMPLETE, even after a finish_reason.

Probe calls `/models`, a minimal one-token completion, and (OpenAI) a one-token
JSON-mode request. Metadata limits are labeled provider-declared, not boundary
measured. Missing limits remain null. JSON parameter acceptance is not proof
of valid output at one token. Prompt overhead is the **total prompt_tokens of
the minimal prompt**, not an isolated measurement of injected system text.
Operator context-window overrides are labeled; no model-name lookup invents
limits. This tool does not infer unlisted limits by making oversized paid calls.

## Builder

Keep the existing loopback endpoint and model id. Select the new explicit
credential-free mode in the pg-builder JSON config:

```json
{"endpoint":{"origin":"http://127.0.0.1:8317","path":"/v1/chat/completions","model_id":"example-model"},"secret":{"mode":"model-egress"}}
```

This fragment belongs in the existing full builder config. It does not replace
its budget/purpose/etc. Builder sends no Authorization header in this mode;
it never reads the upstream key. Existing bearer-file configurations still work.
The existing generate-port/loopback restriction is unchanged.

## Evidence boundaries

Fixtures are synthetic recorded wire transcripts, not captured vendor claims.
SSE fixtures are JSON-escaped strings; replay decodes their exact bytes, including
the required blank event separators.
`live_check.py --config PRIVATE_CONFIG --out PRIVATE_RUN` is explicit opt-in:
two probe completions, one short completion and one stream through an ephemeral
loopback listener. It saves raw responses and usage locally, closes its own
listener, and never retries paid calls. A failure stays a failure.
