# DL01-W1-36 findings

## Verdict

Approved for the sealed-import extension of frozen child DL01-W1-05 at `b1ffb079d7beec4297dbd3deec97219584c6775d`, seal `b0e64601a24d4a824fe30ed32cba038deb811ee7`.

The extension is an own-child revision. It does not open a new acceptance unit. Commit `cb5b71e37` remains the historical approval of source `5bd6e414` and does not cover `compile_sealed_import_catalog`, `compile_reviewed_sealed_import`, or the sealed branch of `compile_source_semantics`.

No product file was edited. The first container's clippy failure was a nested `if` in the review probe. The probe was corrected and rerun after that container exited.

## Schema bytes

Archive `577160c` `contracts/schema/oos-v1.json` sha256 `48432a164cba1e42c28790ad499850a8f3b76abc0b3fb863d9538b6dbe279462`, blob `90b56670f23dea50adfa722e55b8b55296f6f9d4`, 54432 bytes.

`5bd6e414` and `b1ffb079` share sha256 `ee134c232f0d1f1e5ac3cdd435462ee15ed0f66a801dad7d7cdd32daf244f331`, blob `f8bbc8d0f24bb0d0382e778ed399c6946cc6c69e`, 60068 bytes.

The author sealed-import handoff tells the next reader to leave that schema file on the archive. The author's attempt 4 mount used `/tmp/oo-dl01w1-05/overlay-schema`, whose current bytes match `b1ffb079`, and the inside script set `OO_WRITE_SCHEMAS=1` on a writable schema mount. This review overlaid the `b1ffb079` schema read-only, left the variable unset, and the schema contract test passed without rewriting the file. `phases/schema-before.sha` and `phases/schema-after.sha` match.

## What the extension does

Producer catalog sha256 `d9af109ea2f8cd63497771d960a8c396b959501b575b07aa9668fb04a61db844` flows through the three builder APIs into `oo.builder.effective-oos/v1` and a role pack. Provenance records engine `mysql`, source id `mysqlpreview`, config digest `96d9202ffb59c1c0499b87aa191c3d3cf49b517d835ecd2552024f90107a57f7`, schema digest `bac1dde9dc73fc98cf5531621818af2a5d64832a25b6dd60fb52d76942f5cea8`, type digest `e096918bfc0ab782ad06f9177b784974ce217d88b62366ddd8b9da5dc87d28be`, input digest `8434e35cfa49e5dc98e38bf61fd8ff5f019c94c022a3941d3c6eb47c0121e34f`, generations 1 and 3, and identity `33bb4e8b9385da219db0fd784ab06589bda0bbe9c08518fc38fd68fbefe4677f`.

`normalized_representation` is `postgresql_target_storage`. Column `source_type` in the producer document is null, and the rendered native type for `erp.parcel.id` is `absent`. The README sentence is `所有 SQL 为 PostgreSQL 方言，无论数据来自何种源库`.

`compile_captured_pg_catalog` on those bytes returns `field.unknown` / `catalog evidence is not oo.source.pgsql/v1`. A catalog protocol of `oo.source.mysql/v1` returns `protocol.mismatch` and no pack.

Mutations of the binding catalog version, visibility catalog version, schema digest, state `retired`, state `unsealed`, config digest, source id, engine against a mysql target, generation 0, target generation 4, type digest, and input digest produced no pack under the original identity. The altered input digest `a434e35cfa49e5dc98e38bf61fd8ff5f019c94c022a3941d3c6eb47c0121e34f` packs under identity `24563cf2bd58ac70592c7cdb2b5c9883abb6852691f1c14479c8b5b284bebe8b`. That cache key does not contain the producer identity.

Precision-loss key oid 700 blocks with catalog proof `refused`. A precision-loss cursor on `rate` blocks with pack null. Money without a unit is `oos.invalid` at `/columns/5/unit`. Unit `unknown` and `UNKNOWN` are `decision.invalid`. Declared-only foreign keys at no sample, zero rows, and 979 thousandths block. Ratio 980 with 12 rows is a candidate in `## 来源与语义` and is omitted from `## relationships`. Hidden column names, shadow tokens, and `NVL` stay out of the page, README, and verified SQL.

The old w36 probe still passes on this overlay: grades T/S1/S0/B, USD money, precision-loss key and cursor on the PostgreSQL path, the 980 rule, hidden names, and source-dialect SQL.

## Observations that do not change the verdict

The sealed money-unit failure message is `effective OOS validation failed` at `/columns/5/unit`. The code is `oo.builder.sealed-import-draft/v1/oos.invalid`. The PostgreSQL path still returns its own `oos.invalid` unit denial; `w36_profile_grade_and_absent_or_unknown_unit_are_denied` passed.

A sealed cursor block leaves catalog proof `accepted` and then returns status `blocked`. The precision-loss key path sets catalog proof `refused`. Both leave `pack` null and call only `compile_sealed_import_catalog`.

The author's oracle-engine test packs when the target family is changed to match. The independent engine mutation keeps the target family at `mysql` and gets `provenance.conflict`. Neither result is a live database run.

## Author failures retained

Attempt 1 semantics exit 101 at `tests/sealed_import.rs:105`. Attempt 2 clippy-core exit 101. Attempt 3 clippy-builder exit 101. Attempt 4 test exit 0. Those files were not edited.

## Outside this approval

Parent DBX-W05 stays open. Acceptance count stays 0/8. Gateway `get_freshness` is `not_called`. `live_database` is `not_connected`. `live_binary_sort_proof` is `not_run`. DBP8, the support matrix, a 14-night run, and release were not reviewed. `tools/source-conformance` was not overlaid.
