## Card B-S3b — lane B: native Rust parity for login / whoami / share-prompt

TODO: S3-B-01, S3-B-02. Rebase on v0.2/product-loops first; your S3 npm-side commits stay on your branch until the S2 exit.
Your previous report said the standalone Rust commands remain unimplemented and proposed adding reqwest to oo-cli.
oo-sdk already depends on an HTTP client — check what it uses and reuse that (same locked versions) rather than adding a
new client; if a new dependency edge is unavoidable it must resolve to already-locked versions (no new registry versions)
— report it. Implement native `oo login` (enroll code, code-stdin, device flow with slow_down), `oo whoami`, version
negotiation via /v1/meta with cached check and distinct exit code, and `oo share-prompt` (default one-time code; --raw-token
risk line; --agent claude|cursor|codex). The npm wrapper keeps working and delegates to the native binary where it should.
Tests: the existing owned-PG enroll/device/login/query runner on the native binary; expiry warning; version rejection;
prompt contains no long-lived secret by default. Work rule: smallest local workaround and keep going; commit as you go.
