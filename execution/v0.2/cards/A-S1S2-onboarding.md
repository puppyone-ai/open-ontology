## Card A-S1S2 — lane A: oo doctor, oo init TUI, lite compose and oo up/down/status

TODO: S1-A-02, S2-A-01, S2-A-02. Lane label: `a`. Spec: REQ-ONB-01/02/03, AT-ONB-02.
Prerequisites on the integration branch now: your S0/S1 work, the PostgreSQL `SourceEngine` adapter
(crates/oo-connectors/src/postgres), lane H fixtures/oo-test, lane B tokens. MySQL and Oracle adapters are
still on their lane branches. Do NOT stop for that: build everything against the generic `SourceEngine`
interface with an engine registry keyed by `EngineFamily`, wire PostgreSQL now, and leave MySQL/Oracle as
registry entries that return a clear `engine not available in this build` until the coordinator integrates
them (the coordinator will dispatch a short follow-up to wire them). Every exit below is run on PostgreSQL
now; record the MySQL/Oracle parts as pending integration.

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
   certs (2-year) and admin credentials into the 0600 secret dir. Exit: one command from a local PostgreSQL fixture DB (lane H
   fixture via oo-test) runs to an unreviewed draft (Self-build draft produced through model-egress with the configured model).
Write scope: crates/oo-cli/src/admin/**, crates/oo-cli wiring, crates/oo-stack/**, tools/model-egress/**,
deploy/lite/** (new), supply-chain/ new records, root Cargo.toml members/Cargo.lock (report). Prefix `cli`,
`stack`, `deploy`, `build(deps)`.
