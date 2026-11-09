# V3-G-02 acceptance evidence

Run: `v3-g-02-20261109`. All task-owned containers removed. No customer,
external model, public Gateway, source service, or other worktree accessed.
The publisher draft uses the existing deterministic replay model.

## Exits

1. Builder and Python producer hash the sorted decision map and graph, using
   compact UTF-8 JSON. The fixed Unicode vector is identical in
   `crates/oo-builder/tests/adjudication-hash-vector.json` and
   `execution/v0.3/contracts/hash-vector.json`; Rust independently mirrors
   the TUI serializer and Python checks the same constant. Empty TUI overrides
   may carry a null digest; nonempty overrides must carry their exact digest.
2. Aggregate-only personal-name (CJK surnames / Latin given-family patterns),
   street-address, contact-text and context-bound birth-date detectors run
   inside the replica. Name-only negative profiles remain visible. The five
   original fixture canaries are explicit SYN/CANARY-PII markers, recognized
   separately; their success is not presented as semantic-name detection.
3. D28 uses human-authored fixture gold and a measured declared-FK-only
   baseline, not invented historical reviewer actions. L1 owner evidence
   records review counts but no usable per-relation decisions. Fixed baseline
   recall: T6 7/7, Logistics 0/3. Open baseline: T6 7/11, Logistics 0/7.
   Open gold includes fixture-implied joins and the composite dd -> ht key.
   Discovery expands all cross-table key-like column pairs before input
   binding; it is no longer limited to proposed model candidates.
4. The publisher patch is applied. PG17 retained load/prepare -> replay draft
   -> read-only adjudicate -> confirm -> compile -> publish activates a ready
   head. Stale confirmation and modified compiled pack both return exit 2
   before fences/head change. A freshly confirmed override publishes.

## Precision / recall

Each cell is precision / recall. These are per-column or per-candidate
classification scores, not source-engine certification or business accuracy.

| Pool | Dataset | Relation | Sensitive | Metric | Entity key | Code table |
| --- | --- | --- | --- | --- | --- | --- |
| fixed | T6 | 1.0000 / 1.0000 | 1.0000 / 1.0000 | 1.0000 / 1.0000 | 1.0000 / 1.0000 | 1.0000 / 1.0000 |
| fixed | OO-Logistics | 1.0000 / 1.0000 | 1.0000 / 1.0000 | 1.0000 / 1.0000 | 1.0000 / 1.0000 | 1.0000 / 1.0000 |
| open | T6 | 0.0833 / 1.0000 | 1.0000 / 1.0000 | 1.0000 / 1.0000 | 1.0000 / 1.0000 | 0.1250 / 1.0000 |
| open | OO-Logistics | 0.4667 / 1.0000 | 1.0000 / 1.0000 | 1.0000 / 1.0000 | 1.0000 / 1.0000 | 1.0000 / 1.0000 |

Open relation confusion counts (TP/FP/FN/TN): T6 **11/121/0/348**,
OO-Logistics **7/8/0/193**. Open T6 code-table counts: **1/7/0/14**.
Other class counts match the fixed pool. Fixed sensitive counts: T6
**2/0/0/37**, Logistics **3/0/0/65**. Separate semantic profiles:
**7/0/0/7**, precision **1.0**, recall **1.0**. Original format probes pass.

Both D28 commands exit 0. This means the card's recall/nonregression and
sensitive P/R thresholds pass, not that open-pool relation precision is good.
No relation precision threshold was specified or weakened. Coincidental
numeric containment and alternate/reverse key matches are real false positives
under the retained D28 adoption rule. They need disambiguation before this
open-pool output is suitable for unattended production adoption.

## Commands and raw evidence

Use `evidence/v3-g-01-20261109/venv/bin/python` for Python commands and
`evidence/v3-g-01-20261109/logistics` for the seed-28 xs logistics fixture.
Only 7 logistics tables are in this evaluation, not all 18 tables.

- `cargo check --workspace --locked`: pass (`publisher-check.log`).
- `cargo fmt -p oo-builder --check`: pass (`fmt.log`).
- `cargo test -p oo-builder --locked`: 195 pass (`builder-2.log`).
- `cargo build -p oo-builder --bins --locked`: pass (`semantic-build.log`).
- `OO_LIBRARY_BUILDER_BIN=target/debug/oo-builder python -m unittest discover
  -s tools/pg-builder/scripts`: 361 tests, 348 pass, 13 skipped (`python-3.log`).
- `python -m unittest discover -s tools/oo-test`: 185 pass (`oo-test-3.log`).
- `python tools/oo-test/d28_live.py --output evidence/RUN/fixed --logistics
  evidence/v3-g-01-20261109/logistics`: pass (`fixed-1`).
- Same command with `--open-pool`: pass (`open-2`).
- `python tools/oo-test/d28.py --manifest evidence/RUN/POOL/manifest.json
  --output evidence/RUN/result.json`: fixed/open exit 0 (`*-score.log`).
- `python tools/oo-test/d28_publish_live.py --output evidence/RUN/publish`:
  exit 0 (`publish-3/result.json`); PG **17.11 Debian 17.11-1.pgdg12+2**.
- Publisher lib tests: 104 pass, 12 ignored, one environmental failure;
  exclusion rerun passes (`publisher-tests.log`, `publisher-available.log`).

Publisher full-test failure (same on base archive):
`live::private_postgres_activates_only_a_test_scope_candidate` requires absent
`OO_A359_PG_PORT`. Raw proof: `base-publisher-tests.log`.

PG-builder Rust tests (`cargo test --manifest-path tools/pg-builder/Cargo.toml
--locked --tests --no-fail-fast`): 67 pass, five failures, identical on base:

- `document_profile::tests::shipment_catalog_keeps_weight_and_the_nineteen_approved_columns`
- `source_contract::tests::derived_source_bound_projects_native_notes_without_a_forged_grade`
- `source_contract::tests::oracle_batch_and_ts_keep_real_caveats_and_omit_grade`
- `stored_receipts_keep_party_and_account_in_one_pack`
- `fixture_and_stored_generations_are_mixed_scope`

All require missing historical evidence. Proof: `pg-tests.log` and
`base-pg-tests.log`. Base archive is inside this worktree's ignored evidence;
no other worktree was touched. Publisher crate-wide formatting fails on
pre-existing code identically in base; added adapter code is rustfmt-formatted.
No lockfile, dependency, existing migration or unrelated formatting changes.

## Preserved unsuccessful attempts

- `open-1`: socket readiness raced PostgreSQL's temporary startup server;
  changed the harness readiness probe to TCP, then `open-2` passed.
- `publish-1`: fixture CA file mode rejected; make fixture CA owner-private.
- `publish-2`: fixture role authentication failed; use the existing local
  fixture-admin transport with SET ROLE d28_reader, and grant metadata SELECT
  in _oo_load. Actual profiling queries execute in read-only transactions
  under that reader role. `publish-3` passed.
- `python-2.log`: old stubs omitted graph / JOIN metadata query responses;
  update the stubs to the real shape, without removing assertions.

## Limits

No MySQL/Oracle execution, external model drafting, public Gateway acceptance,
interactive TUI round trip, or production-wide semantic-PII accuracy is claimed.
Name lexicons are bounded; generic dates require birth-column context. Existing
compiler entity-key graph updates and dependent-metric demotion after overrides
are not implemented by the TUI reference helper; those override cases still
need coordinator integration testing (the shared preimage itself now matches).
The publisher test uses the compiler's effective graph for its key override.

Shared-file patches requested: none for the card's tested exits. Publisher
adapter applied under the explicit grant. Cargo.lock changes: none.

## Artifact SHA-256

- `fixed-1/manifest.json`: `bf6df7656033519e0b307f1de7f7f61cda5d8b32c4170f211c3db0fd887f191e`
- `open-2/manifest.json`: `a75bca0db2b31dcf0c185047ecfc32790a87933bf139bf4eefd77df1ff4365a1`
- `fixed-result.json`: `70df9855b758f1c8076364ce23d126d61dbb447b05979a0e0ef716a5dc28e98e`
- `open-result.json`: `543ace8cf3025f812cfca36449e484d9377b47759ea528c8914874e724c028a8`
- `publish-3/result.json`: `33de6ddff4a23232bb572eb0675498a60d619ea659d59d52ffe66ef339c4b6be`
