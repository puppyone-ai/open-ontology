# Handoff

Verdict: APPROVE_LIMITED for the sealed RolePack loader at `6001e18ee15d7f6a12d7e5f9c9cd89c8cd8a7b5c`.

E45 stays open. This packet does not accept the parent gateway goal, public TLS, OAuth, an IdP, a readiness database probe, durable audit, SQL execution, or runsc.

Do not treat the suite as all green. `OO_A326_LIVE=1 cargo test -p oo-gateway --test http_gateway_live -- --ignored --locked` fails. `cargo test` without `--ignored` exits 0 because that test is ignored.

## Required test change

Edit only `crates/oo-gateway/tests/http_gateway_live.rs`. Do not edit `pack_catalog.rs`, `fixture_artifact`, the loader, or the compiler.

1. Remove `use oo_gateway::pack_catalog::fixture_artifact;`.
2. Delete the two `fixture_artifact(...).expect(...)` calls in `write_config` (archive lines 235–236) and the writes of those strings to `packs/picker.json` and `packs/auditor.json`.
3. Load compiler-sealed artifacts with the same command `http_gateway_pg_live.rs` `compile_pack` already uses (archive lines 239–264):

```text
OO_A341_FIXTURE_BIN compile
  --compiler "$OO_A341_ROLE_PACK_BIN"
  --effective "$OO_A341_FIXTURE_DIR/effective.json"
  --visibility "$OO_A341_FIXTURE_DIR/visibility-picker.json"
  --output <config>/packs/picker.json
  --digest <config>/packs/picker.json.digest
```

Run it again with `visibility-auditor.json`, `packs/auditor.json`, and `packs/auditor.json.digest`. `OO_A341_FIXTURE_BIN` is `rustc` of `testing/real-db/gateway-compiled-pack` (`src/fixture.rs`). `OO_A341_ROLE_PACK_BIN` is the workspace `role-pack-bundle`. Require both digest files to be 64 lowercase hex characters and require the two digests to differ. Copy the existing `compile_pack` function. Do not add a second sealer.

4. Leave the HTTP assertions in place. The public fixtures already use columns `picker_note` and `auditor_phone`, project `depot`, policy version `pv1`, and knowledge version `kv1`.
5. Leave this file's single 5200ms revoke sleep as it is. Do not add the 1-second warm-hit check here. That check already lives in `http_gateway_pg_live.rs` and is the compiler proof.
6. If the env names stay `OO_A326_*` for the database, add the three `OO_A341_FIXTURE_BIN`, `OO_A341_ROLE_PACK_BIN`, and `OO_A341_FIXTURE_DIR` inputs the compiler command needs. Do not point them at an operator JSON file.

Alternative, if this entrypoint is retired: delete `crates/oo-gateway/tests/http_gateway_live.rs` and name `http_gateway_pg_live` as the only ignored live test. Keeping the file ignored makes ordinary `cargo test` exit 0 while the command above exits 101.

## Proof to keep

`linux/smoke-3` is the fresh run. `linux/smoke` and `linux/smoke-2` are failed reviewer attempts and stay in the tree. Raw exits, host config, and the old-live panic are under those directories. Source hashes are in `source/INVENTORY.txt`. Owned Docker and `/tmp` cleanup is in `CLEANUP.txt`.

The proof container was removed by `launch.sh`. Owned cleanup finished at `2026-10-07T17:32:40Z` and is recorded in `CLEANUP.txt`: container `oo-a345-pg`, volumes `oo-a345-eef831aa-cargo`, `oo-a345-eef831aa-target`, and `oo-a345-eef831aa-pgdata`, network `oo-a345-eef831aa-net`, and `/tmp/oo-a345-eef831aa-dba4-4af2-a78b-541bc9e0a345`. Prune was not run. No foreign container was removed. `oo-a339-proof7` was already absent at cleanup.
