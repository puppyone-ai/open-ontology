## Card A-S1S2 — lane A: oo doctor, oo init TUI, lite compose and oo up/down/status

TODO: S1-A-02, S2-A-01, S2-A-02. Lane label: `a`. Spec: REQ-ONB-01/02/03, AT-ONB-02.
Requires: your S0/S1 work and the engine lanes' S0/S1 adapters integrated (rebase first).

Deliver:
1. S1-A-02: `oo doctor` calls the MySQL/PostgreSQL/Oracle `SourceEngine::probe` and `plan_chunks` for the
   configured stack, prints version, capability states, chunk plan summary, row/byte estimates and the DBA
   GRANT/parameter list; `--json` emits the ProbeReport set. Model probe via model-egress (window, output,
   overhead). Never writes to sources.
2. S2-A-01: `oo init` TUI with ratatui + crossterm (7 steps per docs/product/04 §3), writing exactly the
   same stack file as `--config`; secrets go straight to the 0600 secret store (or keychain), never to the
   stack file, argv, logs or screen after entry. Tests: scripted/pty-driven interaction test producing a
   byte-identical stack file to the non-interactive path; secret scan of outputs. Dependency audit records
   per supply-chain/policy.md for ratatui/crossterm (and anything else new).
3. S2-A-02: deploy/lite: compose file with pinned digests for Agent DB (postgres), control DB, worker,
   Builder, Gateway, model-egress, scheduler; images built from this repo (Dockerfiles you add under
   deploy/lite/) for linux/amd64 and linux/arm64 locally; `oo up/down/status` drive it (status shows last
   success, last failure, next due, nearest credential expiry). First `oo up` generates internal CA + mTLS
   certs (2-year) and admin credentials into the 0600 secret dir. Exit: one command from a local fixture DB
   (MySQL, PostgreSQL, Oracle — use the lane H fixture containers or the engine lanes' test fixtures) runs to
   an unreviewed draft (Self-build draft produced through model-egress with the configured model).
Write scope: crates/oo-cli/src/admin/**, crates/oo-cli wiring, crates/oo-stack/**, tools/model-egress/**,
deploy/lite/** (new), supply-chain/ new records, root Cargo.toml members/Cargo.lock (report). Prefix `cli`,
`stack`, `deploy`, `build(deps)`.
