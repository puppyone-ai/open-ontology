"""oo.doc-parser/v1 sidecar for audio and video (D69, D72-D77, 10 section 17).

Reuses the HTTP surface and supervisor of `doc_parser` (tools/doc-parser). Long jobs: `POST /v1/parse` answers
`202 {task}`; `GET /v1/tasks/{id}` gives `{state, done_ms, total_ms}` and at the end the result or the error;
`DELETE /v1/tasks/{id}` kills the worker (same semantics as a cancelled request in 10 section 2.6).
"""

ADAPTER_VERSION = "1"
PROTOCOL = "oo.doc-parser/v1"
