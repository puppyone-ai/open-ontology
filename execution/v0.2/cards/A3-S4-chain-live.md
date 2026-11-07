## Card A3-S4 — lane A3: `oo build` end to end through publish, Gateway, token and prompt

TODO: S4-A-02. New worktree/branch `v0.2/a-chain` cut from the integration branch (lane A's own worktree is in use by
the remote-deploy executor; do not touch it). Lane M's publish entry is now integrated: `oo-load publish --generation
<id> --library <id>` / `oo_pipeline::engine_publication::publish_generation` plus D15 activation, and a live Gateway
cycle harness `tools/oo-load/test-gateway-cycles.py` (evidence `m-s4d-live-16`). Lane M is concurrently editing
oo-load, pipeline, Gateway and deploy/lite scheduler wiring — do not edit those; call them.
Model config for live runs (authorized, read via secret_ref only, never print/copy): base URL
`~/.config/open-ontology/secrets/model-endpoint-9r`, key `~/.config/open-ontology/secrets/model-key-9r-sol`, model
`gpt-6.1-sol`; bounded calls.
Deliver (commit after each step):
1. Replace the fail-closed stub in `build.rs::publish_reviewed_registry` with: Builder publisher opens the reviewed
   fences → `oo-load publish` activation → Gateway start/reload → first user token (lane B's token store; add an
   idempotency key so an interrupted issuance retries without a duplicate credential — if that needs a token-store
   change, make the smallest one with a test and name it) → `oo share-prompt` printed.
2. `--resume` at every stage boundary including the new ones (SIGKILL tests).
3. Live: one `oo build` on a PostgreSQL 17.11 fixture from empty to a printed share prompt; then use the printed prompt's
   CLI steps (`oo login` with the token, `oo whoami`, one Gateway query through the native CLI) to prove an Agent can
   self-configure. Then MySQL 8.4.7 and Oracle 23.5 `oo build` to the share prompt. Evidence per engine.
