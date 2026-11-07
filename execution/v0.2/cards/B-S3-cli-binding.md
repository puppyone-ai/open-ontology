## Card B-S3 — lane B: oo login / whoami / share-prompt

TODO: S3-B-01, S3-B-02. Lane label: `b`. Spec: REQ-TOK-03/05, AT-TOK-02.
Builds on your token work (integrated); rebase first.
Read: docs/product/04 §5.3–§6, docs/08-agent-interface.md §3.5/§4.3, crates/oo-cli, packages/cli/bin/oo.mjs,
packages/cli/SKILL.md.

Deliver: `oo login --gateway URL (--code CODE | --code-stdin | --device)` (device flow per RFC 8628 with
slow_down +5 s), token written to `~/.config/open-ontology/tokens/<profile>.token` 0600, profile stores
only the reference, post-login `whoami` + one read check; `oo whoami` (identity, role, expiry, protocol;
stderr warning at ≤7 days); `/v1/meta` check cached 24 h, below-minimum CLI → distinct exit code + upgrade
command; `oo share-prompt [--role R] [--agent claude|cursor|codex] [--raw-token]` (default one-time enroll
code; raw token adds a risk line; agent variants add MCP setup via tools/mcp-gateway). Gateway device
endpoints if not done in S1 (approve via `oo token approve <code>`). Tests: end-to-end enroll→login→query
against a test gateway; expiry warning; version rejection; prompt contains no long-lived secret by default.
Write scope: crates/oo-cli/src/{login*,whoami*,share_prompt*}.rs + wiring, packages/cli/**, crates/oo-gateway
auth/device files, tests. Prefix `cli`, `gateway`.
