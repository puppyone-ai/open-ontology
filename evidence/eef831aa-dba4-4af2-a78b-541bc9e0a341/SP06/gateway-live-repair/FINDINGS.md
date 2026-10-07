# Findings

## F1. The maintained entrypoint called the retired helper

a345 ran `OO_A326_LIVE=1` with `--ignored` and the test panicked at `http_gateway_live.rs:235` with `PackError { reason: "legacy_unsupported" }`. `fixture_artifact` ignores its arguments and returns that error. A default `cargo test` leaves the test ignored, so that command exits 0.

This repair removes the import and both calls. The archive used for the pre-edit smoke still has them (`archive-entrypoint.txt`). The overlaid file does not call `fixture_artifact`. `pack_catalog.rs` was not edited, so the helper still returns `legacy_unsupported`.

## F2. Digest files cannot remain in the pack directory

`PackCatalog::load_dir` rejects every name that is not an artifact. `picker.json.digest` fails `artifact_name` and the gateway would exit before `listen.addr` with reason `pack_name`. The frozen contract says the test checks the digest files and then deletes them. The passing `OO_A326_LIVE` run reached `listen.addr` and completed the HTTP assertions, which requires that deletion. The digest strings are the bundle's `pack_digest` values. This test does not recompute them.

## F3. No new product failure in this run

Pre-edit `pack_catalog` (6), loopback (8), and the ignored live test (exit 0) passed on the archive. Focused Clippy exited 0. The repaired ignored test passed in 11.27s. The existing `http_gateway_pg_live --ignored` test passed in 11.43s. `Cargo.lock` was unchanged. The live logs had no `ooct_` or `oost_` prefix.

Workspace HEAD moved from `c23db28dc27f5cf20741dd36e8a8e88129ad1715` at launch to `94f7c1668d89d628b0e3103fc9f26e3f2a1945f6` before the test commit. The gateway, compiler, and lock paths listed in `REPORT.md` did not change in that range. The proof archive stayed `93f5b8aea78d0d745334da0bc5352afdb86412e2`.

## F4. What this run does not show

E45 is still open. The repaired test does not add the 1-second warm-hit check, the tamper campaign, or the hidden-visibility refusal. Those remain in `http_gateway_pg_live.rs`, which passed again here and is not a new campaign. `/ready` returning 200 with `database=not_probed` is the existing assertion, not a PostgreSQL probe. This packet does not claim public TLS, OAuth, an IdP, durable audit, SQL execution, or runsc.
