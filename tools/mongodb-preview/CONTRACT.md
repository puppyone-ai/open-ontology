# mongodb-preview

The customer report uses compat mode `mongodb-native`, the observed MongoDB
built-in role `read`, capture `C5`, consistency `S0`, token order `None`,
write class `R0`, and budget confidence `unknown`. Change streams are not
declared. The driver license flag is `Apache-2.0`. Server SSPL redistribution,
TLS, and Enterprise Audit are not claimed.

`source_admission::admit` is the admission decision. The driver digest must be
the frozen crate checksum
`622f272c59e54a3c85f5902c6b8e7b1653a6b6681f45e4c42d6581301119a4b8`.
A different 64-hex digest can still be admitted by the library and is refused
by this command before any import command. `readWrite`, `root`, `SELECT`,
`reader`, an unknown budget unit, and `R1w` without approval stay denials.

Exit 3 writes the denial and does not start an import. Exit 2 is a bad
argument, an unpinned digest, an arrow file that is not the frozen 16-shipment
batch, or an admission without the shared-import probe. Exit 5 means the
library admitted the report, the frozen arrow matched, and both pinned
commands refused the Mongo manifest:

- `pg-builder import --request REQUEST --out OUT --stderr STDERR`
- `pg-builder import-cards --catalog CATALOG --out OUT`

The manifest protocol is `oo.mongodb-preview.manifest/v1` and the engine is
`mongodb`. The pinned tools accept `oo.mysql-preview.manifest/v1` and engine
`mysql`. Exit 5 is `blocked_shared_contract`, not a sealed import. This
command does not start PostgreSQL, does not call a model, and does not call
`compile_sealed_import_catalog`, `compile_reviewed_sealed_import`, or
`compile_role_pack`.

`--emit-native` writes protocol `oo.pipeline.native-arrow-import/v1` for
engine `mongodb` and returns 0 when that file is written. That return is the
manifest write. It is not a sealed import. The document omits `admitted`,
`admission_bin`, `registry_path`, `registry_sha256`, and `approval`. The
physical command is `pg-builder import-native` from
`d3b3abcff32cc85602d7736eb1340b14813c069b`, with
`OO_NATIVE_ADMISSION_BIN`, `OO_NATIVE_BINDING_REGISTRY`, and
`OO_NATIVE_BINDING_REGISTRY_SHA256` set together. Exit 0 of that command
requires `import.load_state` `awaiting_publish`. The frozen batch stays the
Arrow file `4764e423c574ab54043873d93250596c290ed08ebb93048bee1e9cc64c4c96c8`.

Exit 0 and exit 4 of this product command are not used. A secret ref is a
`secret://` name. A MongoDB URI or a password is rejected before `admit`.
`--customer` is the allowlist: namespace, approved objects, approved fields,
role `read`, and the row budget. `--read-object` of an unapproved collection
exits 2 before the arrow file is opened. `find_sent` stays false.

`--plan-sync --request FILE --out FILE` decides a later sync. The request
protocol is `oo.mongodb-preview.sync-plan/v1`. Exit 0 is `c5_admitted` or
`c1_admitted`. Exit 3 is `cdc_refused` or `blocked`, and the decision file is
still written. Exit 2 is a malformed request. `source_acceptance`,
`model_called`, `import_invoked`, and `generation_created` stay false. The
decision echoes `every_ms` and does not sleep or schedule.

`C1` is admitted only for topology `replica` or `sharded`, with change-stream
privilege, resume support, a known oplog window, an observed delete event, and
transaction metadata. Grade is `T` and the write class is `R1`. Standalone
topology returns `cdc_refused` / `CDC_TOPOLOGY_UNSUPPORTED` and does not
rewrite the method as `C5`. Missing transaction metadata returns
`CDC_TRANSACTION_METADATA_ABSENT` and does not lower the grade.

`C5` is admitted only as an explicit method with `every_ms > 0`, delete mode
`reconcile`, consistency `B`, token order `None`, no snapshot token, quality
`DELETES_LAGGED`, and write class `R0`. The approved field list stays the
original 19, including `weight`. A shorter list is `FIELD_SET_CHANGED`. An
unapproved observed field is `unclassified`, `visible` false, and
`raw_doc_authorized` false. `null` beside `memo`, `pack`, `blob`, or `marks`
is presence, not a mixed scalar. `lane` may be `string`, `long`, or `int`. `fulldoc_claim_complete` is `FULLDOC_NOT_PRODUCED`.
A complete claim while a BSON double is present is also
`PRECISION_LOSS_CONCEALED`. An admitted plan that discloses the double carries
gap `JSONB_DOUBLE_UNENCODED` for owners DL01-W1-05 and DL01-W1-06.
`fulldoc_produced` and `fulldoc_complete` stay false, and `weight` remains a
float64 column.

`--diff-sync` compares two full reads. Exit 0 writes inserts, updates, and
deletes. The published keys are the new read. A tombstoned key that is present
again exits 3 with reason `resurrected`. Neither command starts PostgreSQL or
MongoDB.

`--decode-docs`, `--seal-docs`, `--mutate-set`, `--profile-doc`, and
`--importer-request` seal a full BSON document through the fixed core
`oo_core::jsonb_doc` at `983577c9b89453b08ab4e1a02cab88df78c3e005`. `weight`
is `$numberDouble` from `canonical_number_double`. A bare JSON number is
refused. Absent approved keys stay absent. `hiddenNote` is
`oo.document.bson-jsonb/v1/field.scope` and is not stripped. `--profile-doc`
calls `compile_jsonb_document_profile` and leaves `ready` false.
`--importer-request` builds an `oo.pipeline.jsonb-document-import/v1` probe
for the fixed `631720c5` importer. `source_acceptance`, `generation_created`,
and `import_invoked` stay false. These commands do not start MongoDB.

`--bind-pack` writes a capture receipt `oo.source-admission.capture-receipt/v1`
and an `oo.pipeline.native-update/v1` request for dataset `oo_w141.shipments`.
The receipt hash is the sha256 of the receipt file. Digests are sha256 of the
audit bytes and of the config, hook, catalog, registry, and Arrow bytes this
command writes. Rows with outcome `sealed` are loaded. `scope_refused` rows
stay out of the Arrow file. `weight` stays `$numberDouble` inside `doc`.
`source_acceptance`, `ready`, and `fulldoc_complete` stay false. This command
does not start MongoDB and does not call the loader.
