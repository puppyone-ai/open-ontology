## Card A-S2c — lane A: full lite stack and one command to a draft through oo-load

TODO: S2-A-02 (and the S2 exit "本地 lite 一条命令跑到 draft"). Rebase on v0.2/product-loops first: integrated now —
your TUI/lite foundations (158d80b0a), oo-load (engine-neutral load into the Agent DB), the MySQL/PostgreSQL/Oracle
engines with S2 change capture, lane B token API. Work rule: smallest local workaround and keep going; commit as you go.
1. Bring the full lite compose stack up: Agent DB, control DB, worker, Builder, Gateway, model-egress, scheduler —
   all seven healthy in `oo status` (last success / last failure / next due / nearest credential expiry from /v1/health).
2. `oo build` (draft part): source → oo-load into the lite Agent DB → Self-build through model-egress (configured model
   `gpt-6.1-sol`; endpoint base URL from the 0600 file ~/.config/open-ontology/secrets/model-endpoint-9r and key from
   ~/.config/open-ontology/secrets/model-key-9r-sol, both via secret_ref; never print or commit them) → unreviewed draft.
   Exit: one command from the PostgreSQL fixture, then MySQL 8.4 and Oracle 23 fixtures, each to an unreviewed draft;
   bounded live model usage; record evidence. Publication/review/Gateway token issuance are S4 — stop at the draft.
3. Keep images small and labeled; build one image at a time; free disk is limited (the dispatcher stops you below 8 GB).
