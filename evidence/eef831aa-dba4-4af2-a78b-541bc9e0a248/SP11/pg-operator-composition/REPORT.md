# a248 PG operator composition

Outcome: `reviewed_policy_semantic_refusal`. The live chain stopped before a policy DTO existed. `parent_acceptance` is false. M1-I01 and M1-I02 stay `not_accepted`. This run does not close a component review and does not release.

Fixed archive: `e0704257a1509b968a80a9ccd2f200319d5483ad`.
Evidence: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a248/SP11/pg-operator-composition/`.
No product crate, dependency, or root lock was edited. Registry triples added and removed by the overlay were empty.

## Toolchain and resources

Pre-overlay smoke on the unmodified archive: `linux/smoke/smoke.exit` 0, `docker-run.exit` 0, `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Focused tests included `analyze_pg_cli`, `mapping_reader_access`, `install_column_acl_cli`, `query_pg_cli`, and `reviewed_policy_cli` against bins built from that archive. Live database variables were unset.

Private PostgreSQL 17.10 listened as `oo-a248-pg` with SAN `DNS:oo-a248-pg` and `pg/ports.json` `{}`. `pg/up.exit` is 0. The chain builder was 6442450944 bytes and 1 CPU, `CapDrop` ALL, network `oo-a248-eef831aa-net`, not privileged. Its host exit is 1 and `OOMKilled` is false (`linux/chain/hostconfig.txt`).

Own cleanup removed that PostgreSQL container, the three private volumes, the private network, and `/tmp/oo-a248-eef831aa-dba4-4af2-a78b-541bc9e0a248`. `cleanup/CLEANUP.txt` records the names. A password and private-key scan of the evidence tree, before those secret files were removed, found no password or key body.

## What the chain completed

| Step | Exit | Record |
|---|---|---|
| import before init | 1 | stderr `oo-pipeline: target schema is not ready` |
| init-pg | 0 | `install` bootstrapped, `full_import` installed, `historical_v2` false |
| import-pg | 0 | `awaiting_publish`, source generation 48, target generation 1 |
| catalog-pg, captured-pg-draft, review-pg-draft, reviewed-effective-oos | 0 | stdout saved under `live/steps/` |
| role-pack clerk and peek | 0 | |
| role-pack-bundle clerk and peek | 0 | |
| second bundle write | 1 | `role-pack-bundle: oo.builder.role-pack-bundle/v1/io.destination the output path already exists` |
| overlay build, lock diff, harness Clippy, compiler Clippy | 0 | `linux/chain/phases/` |

The harness reached reviewed-policy only after its publication query returned count `0` and `pg_has_role(oo_pipeline_runtime, oo_target_owner, member)` was false. A mismatch stops at step `publication`. The stop file is a later step.

Source audit after import, `live/source-audit.txt`: writer 0, reader 0, customers 3, orders 3, source reltuples `-1,-1`, reader insert SQLSTATE 42501. No extra customer row was inserted.

The clerk binding sent to `reviewed-policy` used config digest `91864447c06c216d2b31046150d66421749f46ec0ad884aedb485b589b670dd4`, generation 48, schema `shop`, source `a248src`, login `oo_a248_login`. `amount` was on the allow list with `explicit_allow` false, matching pii `none`.

## Where it stopped

`live/stop.json`:

```json
{"detail":"exit 11 stderr=reviewed-policy: semantic\n","step":"reviewed-policy-clerk"}
```

`live/steps/reviewed-policy-clerk.exit` is 11. Stdout is empty. Stderr is `reviewed-policy: semantic`.

`shop.order_doc.amount` in the review artifact catalog is `type_name` `int8`, `typtype` `b`. The effective document confirms `semantic_type` `quantity`. `compatible` in `crates/oo-gateway/src/reviewed_policy.rs` accepts `int8` only as identifier, foreign key, code, or status code. Quantity fails that check after the physical type has already matched, which is exit 11.

## Requirements that did not run

- No reviewed-policy policy array was produced, for clerk or peek.
- `install-column-acl` did not receive a pasted DTO.
- `provision_mapping_reader` was not called.
- `analyze-pg` was not called.
- `query-pg` did not run projections, `count(*)`, star, two-turn, the hidden-column guard, the write SQLSTATE 42501 path, or local JSONL receipts.
- The null `row_filter` omit seam was not observed on a compiler DTO. The harness code that would omit a null key was not reached.

Attempt 1 is preserved at `linux/chain/attempt-1/`. That overlay build exited 101 because `operator_request` took `RoleSpec` by value. The call site now passes `&RoleSpec`. Attempt 2 is the run recorded above.

Semantic publication, natural language, authentication, authorization, a WAL, and a harness mount stay unclaimed.
