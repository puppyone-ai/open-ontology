# G-S5c: composite review passes; gold acceptance remains partial

S5-G-01 is **not done**. All 60 questions were attempted in one unchanged
real-model pass per engine. There were 40 correct, 10 wrong, 2 refused and
8 upstream-timeout failures. No full-fixture T5 or all-P0 acceptance is claimed.

Rebased onto integration `49104eeff01e46b16181519ef196591ef7102186`.
Tested lane HEAD: `45f19a33933fe784e2a98653fde6c41b720701f6`.
No harness, product code, gold value, scoring rule, model budget or secret
reference changed for this rerun. Previous failed evidence remains intact.

## Commands and results

```sh
tools/oo-test/oo-test t6 --engine postgres --run-id g-s5c-pg-reviewed-20261105 --execute
tools/oo-test/oo-test t6 --engine mysql --run-id g-s5c-mysql-reviewed-20261105 --execute
tools/oo-test/oo-test t6 --engine oracle --run-id g-s5c-oracle-reviewed-20261105 --execute
```

All commands exit 1 because not every answer matches gold. Each uses the
default 2,000,000 events, 200 small fixture tables, and reviewed publisher.
The retained build covers only the 12 gold tables. Source-computed answers
agree exactly across all three engines.

| Measured engine/version | Correct | Wrong | Refused | Failed | Builder calls / limit | Agent calls / limit |
|---|---:|---:|---:|---:|---:|---:|
| PostgreSQL 17.11 | 13 | 4 | 0 | 3 | 11 / 21 | 122 / 160 |
| MySQL 8.4.7 | 14 | 2 | 1 | 3 | 11 / 21 | 130 / 160 |
| Oracle 23.5.0.24.07 | 13 | 4 | 1 | 2 | 13 / 39 | 146 / 160 |

The Builder preflights plan 7, 7 and 13 batches respectively, with two repair
calls reserved per batch. Total dispatched Builder + Agent calls: 433.
There are no model retries; currency cost and timed-out call token usage
remain unmeasured.

All three runs reach native owner self-review, reviewed publication
`activated/ready`, 12 opened fences, native share-prompt enrollment,
`oo login`, and verified `oo whoami`. Composite identities are not replaced,
entities are not removed, and no fixture-fence shortcut is used.

## Per-question results

`wrong` below is raw `mismatch`; `timeout` is raw `failed` with
`UPSTREAM_TIMEOUT`. Each run retains `Q01.json` through `Q20.json`,
redacted model responses, native CLI outputs and `result.json`.

| Question | PostgreSQL | MySQL | Oracle |
|---|---|---|---|
| Q01 | correct | timeout | timeout |
| Q02 | correct | correct | correct |
| Q03 | correct | correct | correct |
| Q04 | correct | correct | correct |
| Q05 | wrong | wrong | wrong |
| Q06 | correct | correct | correct |
| Q07 | timeout | refused | correct |
| Q08 | correct | correct | refused |
| Q09 | correct | correct | correct |
| Q10 | correct | correct | correct |
| Q11 | correct | correct | timeout |
| Q12 | wrong | timeout | wrong |
| Q13 | wrong | wrong | wrong |
| Q14 | wrong | correct | wrong |
| Q15 | correct | timeout | correct |
| Q16 | correct | correct | correct |
| Q17 | timeout | correct | correct |
| Q18 | correct | correct | correct |
| Q19 | correct | correct | correct |
| Q20 | timeout | correct | correct |

Every question's raw `null=false` and `truncated=false`: no returned final
answer contains NULL and no truncation was observed. For timeout/refusal,
these flags do **not** establish a non-NULL answer; no usable answer exists.
NULL/empty-note counts Q09/Q10 pass on all engines, without replacing NULL
with zero. Zero questions are `not_run`.

## Diagnoses and ownership

1. **Oracle SourceEngine / typed-load contract (lane E and coordinator).**
   `crates/oo-connectors/src/oracle/engine.rs` `chunk_schema` emits UTF-8
   for every column; its page flush creates `StringArray` for every column.
   `crates/oo-pipeline/src/engine_load_pg.rs` correctly maps that Arrow type
   to PostgreSQL text. Oracle NUMBER/TIMESTAMP therefore reach Gateway as
   text, despite source metadata. The separate typed `oracle/arrow_part.rs`
   path is not used by this SourceEngine path.
   Oracle Q02/Q05/Q08/Q12 show refused SUM(text) and general numeric casts;
   Q08 ends refused, Q05 returns detail rows, and Q12 returns lexically
   ordered customer IDs instead of numeric order. Separate read-only native
   CLI diagnostics return MIN/MAX `100.125,99.125` versus source gold
   `1.125,120.125`, and refuse SUM with `FUNCTION_NOT_ALLOWED`.
   Q02 and Q19 nevertheless score correct because the model fetched the
   complete small amount column and calculated the answer; this is not
   proof that numeric SQL works. Fix the producer/target type contract,
   not Gateway's cast fence. Shared product files were not modified.

2. **Timestamp syntax discoverability (Gateway guard/capability hints).**
   MySQL Q07 refuses after TIMESTAMPTZ literals with `+00` are diagnosed
   as unsupported casts; bare strings then fail signature binding.
   `civil_timestamp` in `crates/oo-gateway/src/sql_guard.rs` accepts `Z`
   or `+HH:MM`, not `+HH`. The generic cast hint does not explain this.
   Independent native CLI queries on the same owned publication reproduce
   rejection of `+00`, while `+00:00` and `Z` each return 60.
   This is not denied data access or proof that all timestamp filters fail.
   MySQL Q08 recovers by reading dates and aggregating the observed ID range.
   The original Q07 refusal is retained; diagnostic queries do not rescore it.

3. **Question/output contract and Agent interpretation (gold owner H,
   dialogue owner G).** PostgreSQL/MySQL Q05 and all Q13 use names where
   gold requires IDs. PostgreSQL Q12 adds customer names. PostgreSQL/Oracle
   Q14 interprets "Composite-key order line quantity?" as detail rows
   instead of the gold joined total 360. These are exact-comparison failures,
   not evidence of broken composite identity. The questions do not specify
   these output shapes. Oracle Q05 additionally encounters the numeric
   defect above. Do not silently relax gold or claim these as passes.

4. **Model upstream availability (egress/provider owner).** All eight
   failed questions have `UPSTREAM_TIMEOUT` usage receipts at the configured
   120-second request timeout. They are not zero answers or SQL failures.
   No acceptance retry or budget expansion was performed.

5. **Closed SQL shapes (Gateway), recovered rather than acceptance blockers.**
   MySQL/Oracle Q20 initially encounter rejected subqueries/set operations;
   the model obtains the correct total through supported queries.
   PostgreSQL Q05 also misuses `describe` with several table names in one
   argument, then recovers. Tool failures remain visible in transcripts.

Diagnostic run: `g-s5c-diagnostic-20261105`. Its queries use only native
read-only CLI access to these owned fixtures, separately from model answers.
It also records source hashes/excerpts and the passing existing guard test:

```sh
cargo test --locked -p oo-gateway --test sql_guard \
  timestamp_comparisons_require_valid_literals_and_catalogued_same_type_operators
```

## Validation and evidence

`g-s5c-validation-20261105` records:

- `cargo check --workspace --locked`: passed.
- Locked builds of CLI, Builder, Gateway, oo-load and release-coordinator:
  passed.
- `python3 -m unittest discover -s tools/oo-test -v`: 68 passed.
- `check_spec.py`, `check_plan.py`, `check_v02_plan.py`: passed.
- `retained_gateway_install` against each owned target: passed.
- Every listed run-manifest file hash verified; source gold files identical.
- Final lane-G Docker container and volume scans empty.

SHA-256 of each `SHA256SUMS`:

```text
576c03569bfbf26b9ddbf6144eadedf0fb047badd5e5cdaa700b2a23e9c7c7be  g-s5c-pg-reviewed-20261105
0498918871fb39ebad53802236ede7b95ba0040c7b01bd6c4d1d9d81473214f6  g-s5c-mysql-reviewed-20261105
ff5cd52445f7e207771e368dd93c0c2c1a4aebad39d754e93dd94ee206fcbc0b  g-s5c-oracle-reviewed-20261105
c205e6254b3b49fe0e0be45138a47180f2aee5bb895c2837895b575e19225221  g-s5c-diagnostic-20261105
```

No Cargo.lock changes, shared-file patch, Rust edits or formatting changes.
No unrelated test failures were waived. No remaining P0 versions,
full-fixture T5, T7 audit, public deployment or business acceptance was
tested. Gateway still reports per-table consistency, CROSS_TABLE_SKEW and
unmeasured source time; correct synthetic answers do not strengthen these.
Only this card's containers were removed; none remain running.
