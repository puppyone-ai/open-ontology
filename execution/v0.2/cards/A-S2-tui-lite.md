## Card A-S2 — lane A: oo init TUI, lite compose, oo up/down/status; doctor for Oracle

TODO: S2-A-01, S2-A-02, S1-A-02 (Oracle wiring). Lane label: `a`. Spec: REQ-ONB-01/02/03, AT-ONB-02.
Integrated now: PostgreSQL and Oracle SourceEngine adapters (Oracle 11g through the thick worker), your
doctor + engine registry, model-egress, tokens. MySQL lands later — keep its registry entry unavailable.
Do the items in this order and commit each when its tests pass; do not stop early while time remains.
1. S1-A-02: register the Oracle engine in the doctor registry; run doctor on Oracle 23ai/21c/11g fixtures
   (lane H oo-test) and record results; implement byte estimates via SourceEngine::estimate_bytes where the
   engine provides it (None stays "unknown").
2. S2-A-01: `oo init` TUI (ratatui + crossterm) — 7 steps from docs/product/04 §3 — producing the same
   stack file as `--config`; secrets straight to the 0600 store or keychain; dependency audit records per
   supply-chain/policy.md for ratatui/crossterm (cargo audit / cargo deny are installed). PTY-driven test
   proving byte-identical output to the non-interactive path; secret scan of outputs/screens.
3. S2-A-02: deploy/lite — compose with pinned digests (Agent DB postgres, control DB, worker, Builder,
   Gateway, model-egress, scheduler), Dockerfiles under deploy/lite/ building from this repo for amd64 and
   arm64, `oo up/down/status` (status: last success, last failure, next due, nearest credential expiry),
   first `oo up` generates an internal CA + 2-year mTLS certs and admin credentials into the 0600 secret
   dir. Exit: one command from the PostgreSQL fixture DB runs to an unreviewed draft using model-egress with
   the configured model (`gpt-6.1-sol`, key via secret_ref file `~/.config/open-ontology/secrets/model-key-9r-sol`,
   bounded live usage). Then the same for Oracle 23ai.
Write scope: as card A-S1S2 (crates/oo-cli admin, crates/oo-stack, tools/model-egress, deploy/lite/** new,
supply-chain records, root members/Cargo.lock — report).
