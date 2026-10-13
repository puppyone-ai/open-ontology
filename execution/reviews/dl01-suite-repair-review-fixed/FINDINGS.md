# Findings

Request changes. Source `0d0dc60bdb1c1d40eab7888428f16dbd53b0e805` and seal `1b5e2c4e950aaab7b0615fcf3258a197ce09f602` close the duplicate-key runtime grant recorded in `c6dc59912c2b2f5dee55832c73e6b43d71801fa2`. The same binary still counts three nightly suite successes for a package that fails to load. That credit misses the original exact-identity exit. The author `CONTRACT.md` cannot waive it. Review `8788aadd2692bbf8abecfe2a01048c53e10cf74e` is overridden by `execution/reviews/dl01-wave1-intake/DL01-W1-27.json`.

## Closed duplicate-key grant

Both key orders now deny before a runtime claim.

`harness/fixtures/dup-service-last-match/package/manifest.json` contains `service_version` `15.2` and then `service_version` `16.9`, with `exact_version` `16.9`. Exit 1. Cell `rev-w37-dup-match` status `failed`. Codes are `version.identity_duplicate` detail `service_version` and `claim.missing_test` detail `no identity-matched package`. `promoted` is false. Verified count is 0. The same package with three suite days records trailing 0 and `suite_runs` 0.

`harness/fixtures/dup-service-last-drift` contains `service_version` `16.9` and then `15.2`. Exit 1. Detail is `service_version`.

`harness/fixtures/dup-exact-last-match` contains `exact_version` `15.2` and then `16.9`, with `service_version` `16.9`. Exit 1. Cell `rev-w37-dup-exact` status `failed`. Detail is `exact_version`.

`harness/fixtures/dup-exact-last-drift` contains `exact_version` `16.9` and then `15.2`. Exit 1. Detail is `exact_version`.

Equal values stay duplicates. `dup-service-same` repeats `service_version` `16.9` and denies with detail `service_version`. `dup-tested-drift` repeats `tested_versions`, the later value `["15.2"]`, and denies with detail `tested_versions`.

Escaped names are compared after JSON string decoding. `dup-escaped` contains `"service_version": "15.2"` and `"\u0073ervice_version": "16.9"`. Exit 1. Detail is `service_version`. `dup-escaped-v` contains `"service_\u0076ersion": "15.2"` and then `"service_version": "16.9"`. Exit 1. Detail is `service_version`.

Nested identity denies. `dup-nested-service` repeats `service_version` inside one identity object. Detail is `service_version`. `dup-nested-identity` contains two identity objects, service `15.2` and service `16.9`. Detail is `identity`. Cell `rev-w37-dup-ident` status `failed`.

`dup-buried` places the repeated `service_version` inside the CC-01 case object. Exit 1. Codes are `version.identity_duplicate`, `claim.manual_assertion` detail `runtime level has no usable package`, and `claim.missing_test` detail `no package`. The case object fails to load, so the manual-assertion code is the load-error path.

## Reproduced coordinator case

Probe `drift` uses distinct keys only. Command `source-conformance check --matrix /harness/fixtures/drift/matrix.yaml --package /harness/fixtures/drift/package`. Exit 1. Cell `rev-w37-drift` `batch_full` status `failed`. Error codes are `version.identity_mismatch` and `claim.missing_test`. Detail is `service_version 15.2 canonical 15.2 != exact_version 16.9` and `no identity-matched package`. `promoted` is false, `matrix_rewritten` is false, verified count is 0.

Probe `drift-nightly` cites that evidence id from three suite rows. Exit 1. Trailing consecutive success days is 0. `suite_runs` is 0. The codes stay `version.identity_mismatch` and `claim.missing_test`.

## Unmet nightly credit

A load failure withholds the runtime claim and still leaves the nightly row in the success count.

`missing-valid-nightly` omits `service_version` from a parseable identity object. Evidence id `rev-w37-missing-valid-0001`. Days `2026-09-01`, `2026-09-02`, and `2026-09-03` have `raw_exit` 0 and origin `suite`. Exit 1. Cell `rev-w37-missing-valid` status `failed`. Codes are `version.identity_missing` detail `service_version`, `claim.manual_assertion`, and `claim.missing_test` detail `no package`. Trailing consecutive success days is 3. `suite_runs` is 3. `manual_runs` is 0. `downgrade_recommended` is false. `matrix_rewritten` is false.

`malformed-nightly` sets `service_version` to the JSON number `16.9` on evidence id `rev-w37-malformed-0001`. Exit 1. Cell `rev-w37-malformed` status `failed`. Codes are `version.identity_malformed`, `claim.manual_assertion`, and `claim.missing_test`. Trailing is 3. `suite_runs` is 3.

`missing-nightly` uses the invalid trailing-comma manifest. Exit 1. Cell `rev-w37-missing` status `failed`. Codes are `evidence.malformed` detail `manifest.json`, `claim.manual_assertion`, and `claim.missing_test`. Trailing is 3. `suite_runs` is 3.

The claim-only probes for the valid missing key, the numeric value, and the invalid JSON already deny the cell. The unmet part is the nightly credit. The loaded duplicate `dup-service-nightly` already withholds that credit.

Required change: withhold nightly suite credit when the cited package fails to load. Keep the distinct-field `16.9` versus `15.2` deny. Keep the duplicate-key deny and its zero nightly credit. This review does not edit `tools/source-conformance` and does not edit author gold.

## Reproduced claim-checker behavior

Schema, hash, and foreign-target mismatches refuse with exit 1: `matrix.schema`, `package.schema`, `evidence.hash_mismatch`, or `evidence.target_mismatch`. A CC-01 raw file whose bytes differ from the recorded digest adds `claim.missing_test` and fails the cell. A valid same-identity experimental package for `rev-w37-keel` exits 0, marks `batch_full` passed, and leaves verified and certified counts at 0. Designed cells without a package exit 0 as `not_required`. A pass on a designed cell is `claim.silent_fallback`. Container evidence on variant `rds` is `cloud.container_scope`.

Nightly accounting on the designed, gap, downgrade, and manual inputs separates the cases the original card requires. Fourteen consecutive suite days on a designed cell are info `nightly.not_promoted` and leave the level designed, with trailing 14 and `suite_runs` 14. A missed calendar day `2026-06-02` is listed, the trailing success count is 1, and `suite_runs` is 2. Two trailing suite failures on a verified cell recommend `experimental` through `nightly.streak_short` and `nightly.failure_downgrade`, with `matrix_rewritten` false. A manual-origin run does not become a suite day. A verified cell with only that run is `claim.manual_assertion` and `claim.missing_test`, `manual_runs` 1, `suite_runs` 0.

Empty `service_version` and a numeric `service_version` are `version.identity_malformed` plus `claim.manual_assertion` and `claim.missing_test`, because the package fails to load. A parseable manifest that omits `service_version` is `version.identity_missing` plus those two claim codes. Unknown grammar (`16.9-ubuntu0`, `16.9 (Debian 15.2-1)`, a leading space, and equal strings on `mysql-compat`) is `version.identity_unknown` and `claim.missing_test`. Patch `16.8`, major token `16`, mysql prefix `8.0` against `8.0.32` and against `8.0.32-log`, Debian body `15.2`, and oracle `19c` against `19.0.0.0.0` are `version.identity_mismatch` and `claim.missing_test`. Suffix form `16.9 (Debian 16.9-1.pgdg120+1)` and mysql `8.0.32-log` claiming `8.0.32` pass experimentally. A postgres target does not apply the mysql suffix rule to `8.0.32-log`. A mysql target does. `map` prints CC-01 through CC-13 and schema `oo-source-conformance/case-map/v1`.

The sealed product matrix, checked without an author nightly fixture, keeps counts candidate 38, designed 181, excluded 8, experimental 0, verified 0, certified 0. Historical PostgreSQL files stay `historical_draft_preview_not_conformance_suite` with `satisfies_cc_suite` false and `promotes_grade` false. A writable copy of that matrix was unchanged after `check`. Its hash stayed `c4fc6698cfb08d74b08fcadbe1f5a6acb2ba41a769905b06328b28f3ab8944b4`.

## Preserved author failures

The seal still holds the failed attempts. Pre-edit smoke is 0. Post-edit attempt 1 `phases/test.exit` and `phases/clippy.exit` are 101, and `journeys/positive.exit` is 127. Attempt 2 `phases/clippy.exit` and `smoke.exit` are 101. Later post-edit and repair exits are green: test 0, clippy 0, smoke 0, repair-drift 1, repair-match 0, and positive 0. Dup-key attempt 1 smoke and clippy are 101. The green dup-key tree is smoke 0, test 0, clippy 0, fetch 0, positive 0, and the duplicate journeys 1, including `dup-missing-valid` 1 and `dup-missing-comma` 1. Those later files do not replace the failed attempt files. This review did not edit them. They are history of the author packet. They are not exits of the binary built here. The new failure of this fixed tree is the nightly credit on unloadable packages.

## Harness history

`history/c6dc5991/missing/package/manifest.json` hashes to `5147f25da320baf282701be958a1597b77469e6f41bbcb36a6002ffaaa3be126`. It has a trailing comma and is not valid JSON. The saved probe codes are `evidence.malformed`, `claim.manual_assertion`, and `claim.missing_test`. `version.identity_missing` was not executed. That copy stays failed harness history. The regenerated `harness/fixtures/missing` manifest is still invalid JSON and the new probe repeats `evidence.malformed`. The valid missing-field probe is a separate file. Its claim denial executes `version.identity_missing`. Its nightly credit is the product failure above.

## Driver digest observation

Digest `abcd` is refused with `package.driver_digest`. Digest `dcad3959b7a83bbf9377a815f40f278ad33b6acb5d9d1e715d4d3af75934643b` with a matching identity exits 0 on `driver-unbound`. The digest is a 64-hex record. It is not bound to the matrix driver name. That observation is separate from the nightly-credit exit.

## Source support

`source_support_consumer.status` is `pending`. `mutable_workspace` is `not-read`. `protocol_commit` names `parse_support_matrix-absent`, source-support `29597079f7feacc6e3934b5c803686fc80567a93`, review `824b20735bd958c4298913838fb832058d17e512`, and archive `577160c22de63adb9b38e0147f3bc2da893c4d9d`. That source-support commit is an ancestor of `0d0dc60bdb1c1d40eab7888428f16dbd53b0e805` and exports `matrix::load`. `parse_support_matrix` is absent from this archive. This review does not add a designed stub and does not fail the child for that absence. Public API consumer integration stays outside this repair.

## Still pending

Matrix integration, per-target suite execution, nightly CI, and any grade promotion stay pending. DBX-W14 stays open. This review does not add a development denominator and does not edit ledgers. The frozen child is not approved.
