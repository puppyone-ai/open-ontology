# clickhouse-preview

The customer report uses compat mode `clickhouse-native`, capture `C4`,
consistency `S0`, token order `PerPartition`, write class `R0`, and the
configured row ceiling of 100000 with estimate confidence `unknown`.

Admission pin `5df40d24eefa4fbcfa00c4415cfcfde5d368db89` admits this report
when the permissions are the attempt-06 `system.grants` rows and the native
profile is the observed `readonly` `1`, `max_execution_time` `8`, and the
`oo_reader` quota interval from the mounted users XML. `SELECT` scopes are
`oo_dl01w40` and the named `system` tables. `SHOW DATABASES` has no scope.
INSERT, a wildcard scope, and a missing profile stay denied by the library.
This tool writes the decision and exits 0 when the library admits it. It does
not relabel the engine as PostgreSQL or MySQL, does not call
`compile_sealed_import_catalog`, and does not call a model.

The driver digest is an argument. The secret ref is a name, not a password.

`scripts/prepare_native_bind.py` writes an `oo.source-admission.binding-registry/v1` file and two `oo.pipeline.native-arrow-import/v1` manifests for a later `pg-builder import-native`. The operator supplies `OO_NATIVE_ADMISSION_BIN`, `OO_NATIVE_BINDING_REGISTRY`, and `OO_NATIVE_BINDING_REGISTRY_SHA256`. The manifest does not name the binary, the registry, or an admitted flag. Column lists come from the attempt-06 capture receipt and keep all 12 fields, including the types the sealed catalog compiler still refuses.

`compile_role_pack` copies `settings.project` into the role-pack manifest. `pg-builder sealed` copies that same field into the operator policy and authority, and copies the catalog `source_id` into the pack receipt binding. The operator requires those project strings to be equal. This tool sets both `settings.project` and `visibility.project` to `clickhouse_268`. The ontology file name stays `clickhouse`. A pack built with the old project literal is not rewritten.

Attempt-06 captures `note` and `label` as Arrow `Binary`, and `code` and `code_or_null` as `Dictionary(Int32, Binary)`. ClickHouse `String` is bytes. `Binary` keeps every byte. `Utf8` is exact only when those bytes are UTF-8; the `00ff` probe stayed `binary_preserved`. `native_arrow.rs` `target_of` stores `binary` and `dictionary_binary` as bytea oid 17, and stores `utf8` as varchar oid 1043. The sealed catalog for those four names is oid 17, typmod -1, with the original source type and an empty caveat. `caveat_agrees` has no token for this binding, so the caveat stays empty. The product-run-05 catalog file and pack digest `a6750ff2b3050dd5934266aedccf4cb41b843266489e421705c7ecf1b64e9639` are not rewritten. The binder reads the generated oid-17 catalog. Its schema digest is `9e9d184e19869e80d3a8910349e8be398a2247d2a04594276a2da6c239ca9348`. A later sealed call on that document produced pack digest `182324dbd09f19804f9898b4997e0be186478c20d43bdd1824a72dfafdb334db`.

`scripts/row_oracle.py` compares an operator result with the source extract. Qty must be kind `int8` and a string. Numeric cells are compared as scaled integers, then cross-checked with `Decimal` precision of at least 76 plus the cell scale. Expected numbers come from the source text. The script refuses to overwrite a file named `independent-verdict.txt`.

`scripts/customer_case.py` reads approved objects, fields, the row budget, the role, and the model id from the customer file. `readings` is one such file, not a branch in the planner. An unapproved object or field produces no source statement. `fixed_string` is refused as undeclared. `datetime64_9_utc` and the array tokens are refused and are not written as success columns. `timestamp_nanos_utc` is oid 1700 numeric(20,0) with an empty caveat: nanoseconds since the epoch, not timestamptz. The secret ref is not copied into the catalog, settings, visibility, or builder config.
