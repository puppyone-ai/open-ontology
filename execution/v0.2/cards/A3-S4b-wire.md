## Card A3-S4b — lane A3: wire `oo build` to the retained publisher and run the loop live

TODO: S4-A-02. Rebase `v0.2/a-chain` on v0.2/product-loops first. Integrated since your last run: lane P's publisher
(`release-coordinator publish-retained --config CONFIG`, stdin protocol `oo.release-coordinator/publish-retained/v1`,
exit 0 activated/replay, 2 refused unchanged, 1 malformed; receipt carries `gateway_document` + Pack; see
tools/release-coordinator/src/retained.rs and tests/retained_cycle_live.py) and lane P's Builder retained data-only path
(tools/pg-builder/scripts/retained_build.py). oo-cli cannot link release-coordinator without a root Cargo.lock change —
call it as a subprocess. Model config as on card A3-S4 (authorized 0600 files via secret_ref; bounded calls).
Commit after each step; do not stop at a milestone.
1. `build.rs::publish_reviewed_registry`: oo-load sync → `oo-load publish` candidate → Builder (retained) → `oo review`
   → `publish-retained` → write `gateway_document` to `reviewed/library/library.json` and the Pack to
   `packs/library.json` → Gateway start/reload (`build_tail::gateway`) → your idempotent first token → `oo share-prompt`.
   `--resume` with SIGKILL tests at each new boundary.
2. Live PostgreSQL 17.11 (lane H's neutral fixture, not a one-column table): `oo build` from empty to the printed
   share prompt; then follow the prompt's own CLI steps (`oo login` with the token, `oo whoami`, one Gateway query
   through the native CLI). Then a data-only source change and `oo build --resume`/cycle → zero model calls → new data
   served. Then MySQL 8.4.7 and Oracle 23.5 to the share prompt and one query. Evidence per engine.
