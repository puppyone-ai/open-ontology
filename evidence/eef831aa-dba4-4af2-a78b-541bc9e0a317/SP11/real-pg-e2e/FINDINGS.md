# Findings

## Proven on this cluster

- One private PostgreSQL 17.10 loaded `shop.party` 2000, `shop.account` 8000, and `shop.entry` 90000. TLS SAN is `DNS:oo-a317-pg`. Autovacuum is off. No host port.
- Source reader insert returned SQLSTATE 42501. Source writer insert of one party row committed nothing: `INSERT 0 1` then `ROLLBACK`.
- One cancellation returned SQLSTATE 57014. `pg_cancel_backend` returned `t`. The watcher log records a single `priv_ok`.
- Inspect sealed `catalog.json`, `captured-draft.json`, and `import-result.json`, then stopped at `awaiting_review`. Config identity `e7490f751339374bf5d7ef3647cb55190a639d603b4f9186ecff65c0335cdd2e` matches the policy binding digest. `source_cut` 317 and `target_fence` 1.
- `import-not-ready` exited 1 with `oo-pipeline: target schema is not ready`. That is the gate `inspect.rs` requires before `init-pg`. The inspect command then exited 0.
- Default identity budget fields include `max_rows` 10000. The entry snapshot of 90000 rows still completed. `live/measurements/import.txt` records both the product row counts and those default fields.
- Marker `甲注评` is absent from the catalog and the captured draft.
- `national_id` remains unclassified. The effective hidden entry is `shop.party` / `national_id` / `classification.absent`.
- Apply spawned children through `reviewed-effective-oos` (exit 0) and stopped at `reviewed-policy-wide` (exit 11, stderr `reviewed-policy: semantic`). Parent stderr is `pg-onboarding: reviewed-policy-wide exit`.
- Failed apply outcome keeps `database_rollback` false and `database_release` false. `database_state` is `changes_may_have_been_applied`.

## Product seam

`base_type` in archive `reviewed_policy.rs` is `bool | int2 | int4 | int8 | text`. `shop.entry.amount` is PostgreSQL `numeric` (oid 1700) with the frozen semantic `quantity`. `physical_type` returns `Semantic` for that name. The CLI maps `Semantic` to exit 11 and the static stderr `reviewed-policy: semantic`.

`int8` plus `Quantity` is a compatible pair in the same function. Changing `qty` would not remove this refusal. `national_id` is outside `binding.allow`, so classifying it would not remove this refusal either. The frozen contract keeps `amount` as `numeric(18,4)` and keeps `national_id` unclassified. This line did not edit either choice and did not edit the product.

A later static condition in the same archive: `query_executor.rs` `oid_allowed` refuses numeric oid 1700. `query-pg` did not run, so that live exit was not collected. The contract's numeric probe is defined to record the real exit and has no expected exit of 0.

## Preserved harness failures

- `linux/focused-attempt-1` through `focused-attempt-3`: `cargo test` exit 101. Edition 2024 compile errors, then a wrong expected account line (`p03999` against the formula `7999 % 2000 = 1999`).
- `linux/focused-attempt-4` and `focused-attempt-5`: tests passed, Clippy `-D warnings` exit 101.
- `linux/focused` is the successor. `focused.exit` is 0.
- `linux/chain` is the one live attempt. It is preserved. It was not rewritten to obtain exit 0.

## Limits

- The immutable `tools/pg-onboarding/tests/live_chain.rs` still hardcodes generation 48. This fixture uses generation 317 and a separate driver under `testing/real-db/pg-e2e`. That driver has no external crates. Its lock is hand-written and was accepted by `cargo test --locked` on the pinned 1.94 image.
- Independent COPY gold was emitted to `live/fixture/*.copy` and loaded into the source. The target physical tables were not copied out. `live/actual/` is empty. Target bytes were not compared with the gold.
- The runtime membership grant is fixture SQL in `testing/real-db/pg-e2e/cluster/grant-runtime.sh`, performed by the host watcher. It is configuration for this cluster.
- Password files and the CA private key stayed under `/tmp/oo-a317-eef831aa-dba4-4af2-a78b-541bc9e0a317/secrets`. They are not in this evidence tree.
- No root manifest edit, no root lock edit, no `cargo update`, no product crate edit. a313 remains the root manifest and lock writer.
- Semantic publication, public authentication, natural language, harness mount, and runsc stay missing. M1-I01 and M1-I02 stay `not_accepted`.
