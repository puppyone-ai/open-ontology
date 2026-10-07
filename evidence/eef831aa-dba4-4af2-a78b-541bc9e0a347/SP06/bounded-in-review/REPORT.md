# REPORT

Session `eef831aa-dba4-4af2-a78b-541bc9e0a347`. Independent review of the bounded literal `IN` admission in commit `6f87942f1c66ba1a9787b7d7194e58a507af63ef`.

Verdict: **APPROVE_LIMITED**.

SP06-W02 stays open. M1-I01 stays open. M1-I02 stays open. The parent goal stays open. This packet adds no counted unit. The a317 numeric integration stays unaccepted, and its completed count stays 0/4.

The reviewed product bytes are the git archive of `6f87942`. Archive `sql_guard.rs` blob `6d7aef7bb95c9550a7e6f1bc7f3f22f7c098de4f`. Observed before the manifest, HEAD `8d6018ed199f9d657ec6ef888e4de36b5e3f1154` still had that blob. `contracts/gateway/sql-guard-v1.md` at that HEAD is blob `0db43b3544307737727a6e74c5deb0741a036a48`. Its bounded-IN amendment describes this behavior and says the amendment text itself is not independent acceptance. This packet is that review. The contract file was not edited.

## Proof

Pre-edit smoke, review harness absent, Linux arm64, Rust `1.94.0`, `oo-gateway` `sql_guard`: 24 passed, lock unchanged, container removed.

Final run on a fresh archive of `6f87942` plus `oracle/review_bounded_in.rs` only. One 6 GiB builder. One private PostgreSQL 17.10. `OO_A175_LIVE` and `OO_A343_LIVE` were unset. `OO_A347_LIVE=1` applied only to the review test. The author `bounded_in` test was not the cargo target.

- `sql_guard`: 24 passed, 0 failed
- `review_bounded_in`: 13 passed, 0 failed, including `live_query_pg_returns_int8_extrema_for_the_exact_in_list`
- `cargo clippy -p oo-gateway --lib --test review_bounded_in --locked --offline -- -D warnings`: finished with no warning
- lock `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474` unchanged

Live gold, written by the review test after the assertions:

```
qty_0=-9223372036854775808
qty_1=9223372036854775807
qty_2=0
null_in=0,2
not_in_null=
not_in=2,7
equal_0=-9223372036854775808
guard_refusals=8
rows_unchanged=yes
executed_contains_or=no
```

`query-pg` stdout for the frozen statement shape returned three int cells as JSON strings: `(0, -9223372036854775808)`, `(1, 9223372036854775807)`, `(2, 0)`. The summary status was `succeeded`, `truncated` was false, and `end_audit_ok` was true. The admin session re-read the same executed SQL and returned the same cells.

Executed SQL, physical names, predicate kept:

```
SELECT pt_86c35c0e75e75dffd2cf5432bde51c0f2c16df124654687f9e715ac55396.pc_e264e1f8e91a54c360fc1e855a15163946b57e0a262171552819df20f439 AS entry_id, pt_86c35c0e75e75dffd2cf5432bde51c0f2c16df124654687f9e715ac55396.pc_3ff26e749bd86c57b4f6c5f7fbc1c71d672dc15a339559785cbbe92fcc2d AS qty FROM _oo.pt_86c35c0e75e75dffd2cf5432bde51c0f2c16df124654687f9e715ac55396 WHERE pt_86c35c0e75e75dffd2cf5432bde51c0f2c16df124654687f9e715ac55396.pc_e264e1f8e91a54c360fc1e855a15163946b57e0a262171552819df20f439 IN (0, 1, 2) ORDER BY pt_86c35c0e75e75dffd2cf5432bde51c0f2c16df124654687f9e715ac55396.pc_e264e1f8e91a54c360fc1e855a15163946b57e0a262171552819df20f439
```

The fixture stored a fourth row, `entry_id` 7 and `qty` 11. `NOT IN (0, 1)` returned 2 and 7. `IN (0, NULL, 2)` returned 0 and 2. `NOT IN (0, NULL)` returned no rows. Eight guard refusals exited 6 with stderr `query-pg: guard` and no audit file.

## Scope

The review covered type and operator resolution, the 1..=32 list bound, hidden columns, subquery, cast, function, parameter, float, and `ANY` closures, the existing byte budget, R9 fold behavior, and logical-to-physical rewrite. Broader SQL support stays outside this verdict.

A later rerun of the same a317 100000-row fixture can build commit `6f87942` and keep `SELECT entry_id, qty FROM shop.entry WHERE entry_id IN (0, 1, 2) ORDER BY entry_id`. This review does not accept that integration.

## Resources

Images, already local: rust `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, postgres `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Linux arm64. No pull. Final builder 6 GiB, 1 CPU, capabilities dropped. PostgreSQL 1 GiB, 1 CPU, server `17.10 (Debian 17.10-1.pgdg12+1)`, autovacuum off, no published host port. Secrets stayed under `/tmp/oo-a347-eef831aa-dba4-4af2-a78b-541bc9e0a347/secrets` and are removed by `cleanup.sh`.

`oo-a346-pg` was left running. Cleanup removes only owner `a347` and this session.
