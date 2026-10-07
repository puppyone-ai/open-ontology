# Test entrypoint contract

Frozen before the `http_gateway_live.rs` edit. This file does not change `pack_catalog.rs`, `fixture_artifact`, the compiler, the root lock, `testing/real-db/gateway-compiled-pack`, or the a341 evidence packet.

Reviewed loader: `6001e18ee15d7f6a12d7e5f9c9cd89c8cd8a7b5c`, blob `9848dcafc4c36b6512240f20fec1b09b1146c02a`. a345 verdict APPROVE_LIMITED. E45 stays open.

## Entrypoint

`crates/oo-gateway/tests/http_gateway_live.rs` stays. The test name stays `issued_token_reads_its_pack_and_failures_stay_closed`. It stays `#[ignore]` and still refuses to run unless `OO_A326_LIVE=1`. Ordinary `cargo test` without `--ignored` does not execute it.

Database inputs stay `OO_A326_PG_HOST`, `OO_A326_PG_PORT`, `OO_A326_PG_DATABASE`, `OO_A326_ADMIN_PASSWORD_FILE`, `OO_A326_RUNTIME_PASSWORD_FILE`, `OO_A326_CA_FILE`, and `OO_A326_MARKER`.

Compiler inputs, read by the copied `compile_pack` helper, are `OO_A341_FIXTURE_BIN`, `OO_A341_ROLE_PACK_BIN`, and `OO_A341_FIXTURE_DIR`. `OO_A341_FIXTURE_DIR` is the public compiler input directory (`effective.json`, `visibility-picker.json`, `visibility-auditor.json`). It is not an operator JSON file and not a hand-written RolePack.

## Sealing

`use oo_gateway::pack_catalog::fixture_artifact` is removed. The test does not call `fixture_artifact`.

`compile_pack` is the function already in `http_gateway_pg_live.rs`. It runs:

```text
OO_A341_FIXTURE_BIN compile
  --compiler $OO_A341_ROLE_PACK_BIN
  --effective $OO_A341_FIXTURE_DIR/effective.json
  --visibility $OO_A341_FIXTURE_DIR/visibility-picker.json
  --output <config>/packs/picker.json
  --digest <config>/packs/picker.json.digest
```

The auditor call uses `visibility-auditor.json`, `<config>/packs/auditor.json`, and `<config>/packs/auditor.json.digest`. `OO_A341_FIXTURE_BIN` is `rustc` of `testing/real-db/gateway-compiled-pack/src/fixture.rs`. `OO_A341_ROLE_PACK_BIN` is the archive `role-pack-bundle`. The fixture writes the bundle's raw `pack` span and the bundle's `pack_digest` string. This test does not recompute that digest and does not add a second sealer.

Both digest files are 64 lowercase hex characters. The two digests differ. Each artifact's `pack_digest` string equals its digest file.

## Pack directory before listen

`PackCatalog::load_dir` returns `pack_name` for every directory entry that is not an artifact name: a `.json` suffix and a stem of at most 63 characters starting with a lowercase letter or digit, then lowercase letters, digits, `_`, or `-`. `picker.json.digest` fails that rule, so the gateway would exit before `listen.addr` if the digest file stayed in `pack_dir`.

After the digest checks, the test deletes `picker.json.digest` and `auditor.json.digest`. The fixture already deletes its temporary bundle and request files. The directory then contains only `auditor.json` and `picker.json`. Those two files are the compiler spans, not a rewrite.

## HTTP behavior that stays

Every existing assertion stays, including health, picker metadata (`picker_note` present, `auditor_phone` absent, project `depot`, role `oo_r_picker`, scope `client`, `authentication=loopback-http`, external mTLS / OAuth / IdP false, protocol `oo.builder.role-pack/v1`), forged `X-OO-Role` / `X-OO-Project` / `X-OO-Scope` returning 403, session token 401, missing bearer 401, query string 400, `POST /v1/sql` 501, revoke then the one `5_200` ms sleep then 401, restart 401, auditor metadata after restart, endpoint `127.0.0.1:1` then the existing outage `5_200` ms sleep then 401, `/ready` with `database=not_probed`, restore 200, and the stderr / argv secret checks.

No second revoke sleep. No 1-second warm-hit check in this file. No tamper campaign, no hidden-visibility campaign, and no new auth, grant, or SQL. Those checks remain in `http_gateway_pg_live.rs`.

Token issue stays `harness_install` / `harness_issue` / `harness_revoke_token` with the existing `IssueGrant` fields: project `depot`, policy `pv1`, knowledge `kv1`, audience `https://gateway.loopback.invalid/v1`, principals `picker-user` and `auditor-user`, region claim `north`.
