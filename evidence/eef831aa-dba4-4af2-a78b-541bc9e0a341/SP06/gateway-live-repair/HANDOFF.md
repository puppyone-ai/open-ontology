# Handoff

Test repair commit: `99e93d0f88eb95d8b3834c534dc8ae90d62753ec`.

E45 stays open until an independent review of this commit. This packet does not accept the parent gateway goal, public TLS, OAuth, an IdP, a readiness database probe, durable audit, SQL execution, or runsc.

## What changed

Only `crates/oo-gateway/tests/http_gateway_live.rs`. `OO_A326_LIVE` is still required and the test is still ignored. `write_config` calls the same `compile_pack` command as `http_gateway_pg_live.rs`, using `OO_A341_FIXTURE_BIN`, `OO_A341_ROLE_PACK_BIN`, and `OO_A341_FIXTURE_DIR`. The inputs are `effective.json`, `visibility-picker.json`, and `visibility-auditor.json`. Digest files are checked and then removed from `pack_dir`. The HTTP assertions and both `5_200` ms sleeps are unchanged.

`fixture_artifact` still returns `legacy_unsupported`. Do not make it emit a pack.

`pack_catalog.rs`, the compiler, the root lock, `testing/real-db/gateway-compiled-pack`, and `evidence/.../gateway-compiled-pack` were not modified.

## Proof

`linux/smoke` is the run. `docker-run.exit=0`. Raw exits, host config, and markers are in that directory. `recipes/ORIGIN.md` records the a341 recipe hashes and the changes. Owned Docker and `/tmp` cleanup is in `CLEANUP.txt`. The proof container was removed by `launch.sh`.

## Review boundary

Review the test diff against `CONTRACT.md` and the a345 handoff. The loader approval remains a345's `APPROVE_LIMITED` of `6001e18ee15d7f6a12d7e5f9c9cd89c8cd8a7b5c`. A green `cargo test` without `--ignored` still does not execute this test.
