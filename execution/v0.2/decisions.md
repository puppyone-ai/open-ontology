# v0.2 decision log

Append-only. One line per human instruction or scope change, with its effect.
Current task state lives in [state.json](state.json); the plan is
[product-loops-v0.2-plan.md](../../specs/product-loops-v0.2-plan.md).

| Date | Source | Decision | Effect |
|---|---|---|---|
| 2026-11-03 | Human | Product side proceeds separately from customer delivery; three loops: open-source onboarding (CLI/TUI, no Web UI), Gateway↔CLI token binding, MySQL/Oracle/PostgreSQL multi-version first with 10–20 engines in the same schedule | Plan §0, §4 |
| 2026-11-03 | Human | Docs, check scripts and source baseline may be changed directly; design changes are backfilled in the same period, not deferred | Plan D7; baseline revision 6 |
| 2026-11-03 | Human | Do not buy the cloud test server yet | Plan D1: amd64-only engines run under local Docker Desktop emulation |
| 2026-11-03 | Human | Full authority for database prerequisites via the browser; no more questions | Plan D2/D5; account creation and password entry remain the human's own action |
| 2026-11-03 | Human | Test model `gpt-6.1-sol` on the provided OpenAI-compatible endpoint; key kept outside the repository | Plan D4 |
| 2026-11-03 | Human | Keep npm package name, bump version | Plan D3: `@reduxxx888/ontology-cli@0.2.0` |
| 2026-11-03 | Human | Commit directly with clear history; plan states commit scope and standard; one final PR for review after all loops converge | Plan §10, §11 |
| 2026-11-03 | Coordinator | Owner self-review accepted as the publication gate for single-owner deployments, recorded as `mode=self` | Plan D6 |
| 2026-11-04 | Coordinator | Lane A (onboarding: stack schema, admin CLI skeleton, model egress) starts in S0 instead of waiting for a free slot; S0–S1 concurrency is 6 | Plan §2.1; L1 critical path |
| 2026-11-04 | Human | Resume: drive the whole plan to convergence and testing this round; at most 2 native subagents; local Codex (`cx/gpt-6-astra[1m]`, reasoning medium, dangerously-bypass mode) as executors via long task cards and sign-off; main thread owns shared parts | `execution/v0.2/dispatch.sh` and `cards/`; ledger E021 |
