# Findings

No request for changes. The fixed child meets the frozen claim-checker exit on the sealed source, with independent inputs.

## Reproduced behavior

Schema, hash, and source-target mismatches refuse with exit 1 and a single error code: `matrix.schema`, `package.schema`, `evidence.hash_mismatch`, or `evidence.target_mismatch`. A valid experimental package for `rev-w27-keel` exits 0, marks `batch_full` passed, and leaves verified and certified counts at 0. Designed cells without a package exit 0 as `not_required`. A pass on a designed cell is `claim.silent_fallback`. Container evidence on variant `rds` is `cloud.container_scope`.

Nightly accounting on these inputs separates the cases the card requires. Fourteen consecutive suite days on a designed cell are info `nightly.not_promoted` and do not change the level. A missed calendar day is listed and breaks the trailing success count. Two trailing suite failures on a verified cell recommend `experimental` and leave `matrix_rewritten` false. A manual-origin run does not become a suite day; a verified cell with only that run is `claim.manual_assertion` and `claim.missing_test`.

The sealed product matrix, checked without the author's nightly fixture, keeps counts candidate 38, designed 181, excluded 8, experimental 0, verified 0, certified 0. Historical PostgreSQL files stay `historical_draft_preview_not_conformance_suite` with `satisfies_cc_suite` false and `promotes_grade` false. A writable copy of that matrix was unchanged after `check`.

## Preserved author failures

The seal still holds the failed attempts. Attempt 1 `phases/test.exit` and `phases/clippy.exit` are 101, and `journeys/positive.exit` is 127. The attempt 1 aggregator `phases/journeys.exit` is 1. Attempt 2 `phases/clippy.exit` and `smoke.exit` are 101. Later green exits do not replace those files. This review did not edit them.

## Observation inside the published contract

`identity-drift` exited 0. The package says `exact_version` `16.9` and `identity.service_version` `15.2`. `tools/source-conformance/src/package.rs` reads `service_version` at the identity object and stores it on the package. `tools/source-conformance/src/evaluate.rs` does not compare that field to `exact_version`. `CONTRACT.md` requires the identity object and requires `tested_versions` to equal `[exact_version]`. The equality of `service_version` and `exact_version` is not a frozen failure. The probe is recorded so a later contract can add it. It is not a defect against this child.

## Review harness retry

The first launch exited 4 before Docker. The verifier treated `tools/source-support` inherited on the shared source commit as if it were inside the dependency archive. The dependency commit does not contain that directory, and no builder started. The verifier note is `linux/harness-retry/`. The collected run is the second launch.

## Still pending

Matrix integration, per-target suite execution, nightly CI, and any grade promotion stay pending. DBX-W14 stays open. `source_support_consumer.status` stays `pending` until a coordinator publishes an immutable source-support SHA. This review does not add a development denominator and does not edit ledgers.
