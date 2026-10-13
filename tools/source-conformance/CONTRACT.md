# source-conformance

Standalone checker for `oo-support-matrix/v1` claims. It prints a JSON report and does not write the matrix, promote a grade, or open a database.

`verified`, `experimental`, and `certified` are runtime claims. `candidate`, `designed`, and `excluded` do not require suite cases. A supplied package cannot raise a non-runtime cell.

## Commands

```
source-conformance check --matrix FILE [--historical-pg DIR] [--nightly FILE] [--package DIR]...
source-conformance map
```

Exit 0 means the report has no error findings. Exit 1 means a claim or evidence finding. Exit 2 is usage. stdout is JSON for exit 0 and 1.

## Report `oo-source-conformance/report/v1`

`promoted` and `matrix_rewritten` are always false. `counts` are matrix levels, not recommendations. `recommended_level` stays at the matrix level unless a verified or certified cell has two trailing suite failures or a budget/cycle failure, in which case the recommendation is `experimental` and the matrix is still not rewritten. A 13-day streak is `nightly.streak_short` and does not lower the recommendation. Manual-origin runs are not suite days. Missed calendar days break a success streak and do not reset the trailing failure count.

`source_support_consumer.status` is `pending`. Immutable source-support `29597079f7feacc6e3934b5c803686fc80567a93` is accepted by review `824b20735bd958c4298913838fb832058d17e512`. Its public matrix function is `source_support::matrix::load`. The consumer interface `parse_support_matrix` is not that function, so this crate still parses YAML itself. It does not read the mutable `tools/source-support` workspace and does not report a stub integration.

```
source_support::matrix::parse_support_matrix(yaml: &str) -> Result<SupportMatrix, MatrixError>
Target { id, family, product, versions, variant, capabilities: BTreeMap<canonical id, Level> }
```

## Evidence package `oo-source-conformance/evidence-package/v1`

Required objects are `identity` (`topology`, `service_version`) and `cloud` (`applies`, `budget_exceeded`, `cycle_status`). `tested_versions` must equal `[exact_version]`. `identity.service_version` is the actual engine identity. It must canonicalize to the same exact token as `exact_version` before the package can support a runtime claim. A mismatch is `version.identity_mismatch`. A string outside the frozen grammar is `version.identity_unknown`. A missing `service_version` is `version.identity_missing`. A non-string `service_version` is `version.identity_malformed`. A repeated `exact_version`, `tested_versions`, `identity`, or `service_version` key is `version.identity_duplicate` before any runtime claim or nightly success. The check walks every object. Escaped spellings compare after JSON string decoding, so key order cannot select a value. Equal duplicate values stay ambiguous. Those packages do not cover CC cases and their evidence ids do not count as nightly successes.

Frozen grammar, and nothing else: `postgres` accepts an exact token or `EXACT (BODY)` when BODY names that same dotted token once; `mysql` with product `MariaDB` accepts an exact token or the suffixes `-MariaDB` and `-MariaDB-log`; other `mysql` targets accept an exact token or the suffixes `-log` and `-debug`; `mysql-compat` and `postgres-compat` have no product-version normalizer; every other family accepts only an exact token equal to the claim. A shorter token is not a match (`16` against `16.9`, `8.0` against `8.0.32`). `19.0.0.0.0` is not an alias of `19c`.

Raw paths stay under `raw/` and must match `raw_sha256`. A runtime capability needs every applicable CC case with disposition `pass` and raw exit 0, plus one rejection of a non-runtime capability whose code is `CAPABILITY_UNVERIFIED`, `silent_fallback` is false, and raw exit is not 0.

Designed and candidate cells accept ranges such as `13-17`. Verified and certified require every matrix version to be exact, and the union of contributing package `exact_version` values must equal that set. Experimental accepts an exact package version against a matrix range. If the matrix lists any exact token, the package version must be one of those tokens.

Managed variants (`rds`, `aurora`, `cloud-sql`, `alloydb`, `azure-sql`, `polardb`, `serverless-cloud`, `saas`, `aws`, `managed`) cannot use `container` evidence for a runtime claim. Verified and certified require `deployment_scope: public`, three reviewers (`data`, `dba`, `security`) with real names, and 14 trailing successful suite days. Certified also needs every `FAULT-POSITION-EXPIRED`, `FAULT-FAILOVER`, `FAULT-DDL`, `SCALE-S100K`, and `SCALE-5M` case, and evidence type `customer` or `managed_cloud`.

## Nightly registry `oo-source-conformance/nightly-registry/v1`

Each entry is `target_id`, `capability`, `runs`. Each run is `calendar_day`, `raw_exit`, `evidence_id`, `origin` (`suite` or `manual`). The same suite day keeps a non-zero exit. A span above 400 days is `nightly.span_too_long`. A suite success counts only when that evidence id belongs to a loaded package whose service identity matches the claimed exact version and whose required raw cases for the capability pass their recorded hash and raw exit 0. A package that is absent, not JSON, missing or non-string in `service_version`, duplicated, or otherwise not identity-matched contributes no suite success and no success streak. An evidence id that names no supplied package keeps the registry day, so a matched package's separate calendar and a matrix checked without a package keep their successes, gaps, manual runs, and failure counts. Those successes do not promote a designed cell and do not rewrite the matrix.

## Historical PostgreSQL draft

`--historical-pg` reads only `REPORT.md`, `HANDOFF.md`, `six-final/manifest.json`, and `six-final/01.exit` through `06.exit`. Protocol `oo.pipeline.local-pg-mirror-manifest/v1` is classified `historical_draft_preview_not_conformance_suite`. `satisfies_cc_suite` and `promotes_grade` stay false. Physical column names are not copied into the report.

## Scope report `oo-source-conformance/scope-report/v1`

```
source-conformance scope --repo DIR --dispatch FILE --intake FILE --matrix FILE [--frontier FILE] [--claim FILE] [--package DIR]...
source-conformance audit --raw FILE
source-conformance activate --scope-report FILE
```

`scope` reads the dispatch `latest_author_delivery` commits and the intake receipt. It prints one report and writes nothing. `sources_signed_off` stays false. `accepted_sources` stays 0. `promoted` and `matrix_rewritten` stay false. `manufactured_nightly_days` stays 0. Exit 0 means the report has no error findings. Exit 1 means a claim, drift, missing delivery, or a raw result that must not be treated as a pass. Exit 0 is not source acceptance.

Each of the ten products gets CC-01 through CC-13. SQL Server with no opened endpoint is `notrun`. Other current deliveries are `not_probed`: a historical report, a raw exit of 0, an ELF identity, an author `source_bound` boolean, an empty lease, or a self-written receipt is not a probe of this capability. A controlled preview is not a whole source. Dispatch and intake SHA disagreement is `evidence.sha_drift`.

Publish `DL01-W1-75`, Gateway `DL01-W1-76`, and the native-loader rereview `DL01-W1-77` are opened at the dispatch commit. `REQUEST_CHANGES`, a missing agent dialogue, an expired position, a known failure with no new batch, and a debug ELF that is not an author release stay at that original exit. Update/delete intervals and Self-build S0–S10 YAML are not upgraded from a pending or running dispatch status. CLI exit 0 is `claim.cli_exit_not_a_test`.

A scope claim that asserts all-source acceptance, an inline owner/DBA/14-day attestation, an unknown source, an unverified day, a missing object, a blob mismatch, a non-zero raw file labeled pass, producer self-trust, or an audit parser error is an error. Those findings are not rewritten to pass.

`degradation` is a proposal. A verified or certified cell with no passing package this cycle proposes `experimental`. Other cells keep their matrix level. `unverified_this_cycle` is true. The matrix file is not written.

`ci_activation.commands` is empty unless a later gate has a real passing package. `activate` refuses a report whose command list is non-empty or whose CC row says `pass`. An empty activation is not a suite run. The command template remains `source-conformance check --matrix FILE --package DIR`.

`audit` parses `oo-source-audit/v1` statement records for the engine list in docs/database-support/05. R0/R1 reads can be `zero_write`. A write verb labeled R0/R1, R1w, R2/R3, an unknown engine, a JSON `source_bound`/`zero_write`/`trusted` boolean, or a null lease is an error. `product_accepted` and `counts_as_cc02_pass` stay false.

DBX-W14 stays open. External 14-day nights, an external owner, an external DBA, the SQL Server endpoint, and a managed-cloud budget stay pending. This tool does not sign off the ten sources.

## Case map

CC-01 and CC-02 and CC-11 apply to every capability. CC-03 applies to catalog, batch_full, batch_incremental, and cdc. CC-04 applies to batch_full. CC-05 applies to batch_incremental. CC-06 and CC-09 apply to change_feed and cdc. CC-07 applies to delete, delete_native, and delete_reconcile. CC-08 applies to budget. CC-10 and CC-12 apply to catalog. CC-13 applies to family `document` or capability `document`.
