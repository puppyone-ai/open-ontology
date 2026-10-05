# Handoff

Verdict: API slice completed. The role-pack compiler seam stays open. Parent acceptance stays false.

Session: `eef831aa-dba4-4af2-a78b-541bc9e0a203`.
Fixed source: `0ee7cf86ad443ee1d84fe9fe3ed3f7d8847799c1`.
Path: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a203/SP11/pg-vertical-integration/`.
Harness source commit: `87b33812c24b34c3864cb6607fd98bfa43f65a15`.

`live/outcome.json` status is `api_slice_completed`. Run `7dba3fda-8dba-4cbd-83a6-a7ac0382bda0` is `awaiting_publish`. `pg_publication` count was 0. `publication` is `unavailable`. `pg_catalog_integration` is `not_accepted`. `shadow_marked_published` is false. `query_cli` is `not_exercised`.

The approved path that ran: source inspect and catalog, `prepare_snapshot` generation 1, `install` once (`bootstrapped`), `install_full_import` once (`installed`), `import_unpublished`, `compile_captured_pg_catalog` with Chinese dataset labels, structural accept, two `install_column_acl` results (`Installed`), two `load_policy_catalog` loads, and `DedicatedSession::execute_trusted_select` for select, count, and star on `oo_r_party_read` and `oo_r_invoice_read`. Typed rows matched the fixture. See `REPORT.md` and `FINDINGS.md`.

## Seams to leave open

`compile_role_pack` rejected the structure draft (`schema.rejected` at `/`) and rejected compiler datasets (`classification.unknown_pii` at `/datasets/0/columns/0`). A later builder task owns a compiler from a reviewed structure draft to `oo.builder.effective-oos/v1` once a reviewed contract sets PII and classification. This slice did not add that compiler and did not change product code.

Column ACL used the bridge recorded in `live/column-acl.json`: trusted administrative classification, because the ontology compiler left `pii` and `classification` unset. Hidden logical columns and writes were guard refusals. Direct role statements returned SQLSTATE 42501; that SQLSTATE is required by the harness and is not copied into `outcome.json`. `run.exit` is 0.

The relationship item `relationship/ent.invoice/invoice_party_fk` was accepted. A curated override of the same item id was blocked by one review per item id. The effective relationship status is `draft`.

CLI acceptance waits for a fixed a198 commit. Do not treat this API slice as M1 goal acceptance or a release.

## Evidence

- `REPORT.md`, `FINDINGS.md`, this file.
- Harness source is commit `87b33812c24b34c3864cb6607fd98bfa43f65a15`: `Cargo.toml`, `Cargo.lock`, `src/`, `fixture/source.sql`, and the linux and pg scripts.
- `linux/phases/` is the pre-edit Rust 1.94 pipeline and gateway lib smoke. Exit 0. Attempt 1 is `linux/attempt-1/` (pipeline lib exit 101, contracts missing from that archive).
- `linux/harness-phases/` is the successful harness: build, clippy-component, clippy, lock-diff, run, and secret-scan all exit 0. Resolve mode `standalone`. Registry additions empty. Attempts 1–3 are under `linux/harness-attempt-1/`, `linux/harness-attempt-2/`, and `linux/harness-attempt-3/`.
- `live/` holds the catalog, drafts, review queue, role-pack attempts, column ACL, policy catalog view, oracle, and outcome. No `blocker.json`. `linux/harness-run.log` is empty.
- `linux/post-run-harness.txt`, `linux/post-run-smoke.txt`, and `pg/post-run-inspect.txt` are the inspects taken before removal.
- `SHA256SUMS` lists every owned file except itself. `SHA256SUMS.sha256` is the hash of that manifest.
- Password files stayed under `/tmp/oo-a203-eef831aa-dba4-4af2-a78b-541bc9e0a203/secrets` and were removed with that directory. They are not in this tree.

Two commits, owned path only, no push:

1. `87b33812c24b34c3864cb6607fd98bfa43f65a15` `evidence: add the a203 vertical integration harness`
2. `evidence: record the a203 vertical integration run`

## Cleanup

`cleanup/run.log` records the removal. The first `docker rm` of `oo-a203-pg` exited 1 because the container was still running. `docker stop oo-a203-pg` then exited 0, and the following `docker rm`, `docker volume rm oo-a203-pgdata`, and `docker network rm oo-a203-net` exited 0.

Removed containers: `oo-a203-harness`, `oo-a203-rust194`, `oo-a203-pg`.
Removed volumes: `oo-a203-cargo`, `oo-a203-target`, `oo-a203-pgdata`.
Removed network: `oo-a203-net`.
Removed directories: `/tmp/oo-a203-fixed` and `/tmp/oo-a203-eef831aa-dba4-4af2-a78b-541bc9e0a203`.

Final counts in the log: `after_container_lines=0`, `after_volume_lines=0`, `after_network_lines=0`. No prune. No other task's containers or volumes were named in the cleanup commands.
