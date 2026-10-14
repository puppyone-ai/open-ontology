# mysql-preview

Experimental MySQL 8 batch snapshot into the existing PostgreSQL full-import
target. `run` is that snapshot and is not CDC. `c1-prove` reads a ROW/GTID
binlog on an owned MySQL 8 fixture through `mysql_async` and the public
`cdc-transaction` / `cdc-spool` crates. `c1-native` uses that same reader for
the five `native_mysql_20261012` tables and the current role's visible
columns. Neither command applies the PostgreSQL loader or ACKs. The `oo_c1`
fixture is not the five-table product. This is not a verified or certified
connector, and not MariaDB, RDS, Aurora, or TiDB.

The tool is a separate workspace. It calls `oo_pipeline::target_installer`,
`plan_physical_mapping`, `CopyEncoder`, and the `_oo.full_import_*` functions
already implemented for PostgreSQL. It does not rename the PostgreSQL mirror.

Snapshot sessions are one read-only business connection. The snapshot path
does not `SET` the source and does not trial-write. `c1-dba` may `SET` binlog
prerequisites only when the config fixture is `owned-mysql-8` and the server
version is 8.x. Customer RDS 5.7 is not a target of that command.

JSON is stored as PostgreSQL `json` (oid 114) text because `copy-encoder/v1`
rejects `jsonb`. MySQL `TIME` is a signed microsecond duration, stored as PostgreSQL
`interval` (oid 1186), including negative values and values beyond one day.
Values beyond the MySQL TIME domain are rejected. `SET`, spatial, vector, and bit columns are rejected before a
target batch. `tinyint(1)` is `int2` unless `boolean_tinyint1` is true.

Column grants for the gateway role are not issued here. After a seal, the
existing `install-column-acl` and `provision-mapping-reader` commands own that
step. A table-level `GRANT SELECT` would expose `_oo_pos`, which those commands
refuse.

## Manifest

`mysql-preview manifest --access PATH --seal PATH --out PATH` writes
`oo.mysql-preview.manifest/v1` from a captured access outcome and a sealed
outcome. It does not open MySQL. Identity, digests, row counts, and the
canonical digest come from the seal. Caveats come from the access document.
The schema preimage is the same byte string the import uses. A mismatch is
`catalog schema_digest`. Physical names and SQL text are not fields.

A successful `run` also writes `manifest.json` next to `outcome.json` when
the target state is `awaiting_publish`.

## Quality

A batch with no masked rejects still uses the historical preimage:
`full-import/v1 quality` and one `batch` line whose rejected count is 0.
A masked nullable `zero_date`, `time_range`, or `bool_domain` is also a
`reject` line: reason, value length, attribute number, row index, and the
canonical primary-key digest when the assembler produced one. The digest is
64 lowercase hex characters. It is empty only when that digest is absent.
The rejected value is not stored. `write_batch` receives that count and
those arrays. A historical seal that stored rejected 0 does not gain lines
after the fact. Its manifest sets `payload_includes_reject_lines` false and
leaves length and attribute number absent. That is a recorded omission, not
a claim that the input was valid. Those old lines are not evidence for a
later copy.

A MySQL `DATETIME` year outside 1000..=9999 is not a zero date. The DBA
fixture stores SQL NULL and a separate `mysql_datetime_range` ledger row.
The business connection does not write that ledger.

## Scale commands

These commands are the synthetic logistics fixture. They are not the
five-row prototype and they do not open that seal for another copy.

`dba-load` uses protocol `oo.mysql-preview.dba-load/v1` and the setup
account. It loads `logistics` only. `scale-guard` checks the prototype
counts and that the business grants are `SELECT`. `scale-audit` reads with
the business account and compares formula cells, raw `0000-00-00`, raw
negative `TIME`, and checksums. `scale-target` reads the sealed PostgreSQL
copy with the administrator and reads `_oo.full_import_quality` for that
new run. Question text is a separate file and contains no expected cells.

`stop` value `partial_after_first` reads and notes table index 0, leaves
the run `open`, and exits 9 after writing that outcome. `cancel_before_batch`
and `fail_before_seal` retire the run. None of those three states is a
sealed import. The outcome `quality` array lists stored batch digests only
when `quality_stored` is true. A cancel writes an empty array.

## Fresh connection entry

`native-producer.py` takes source identity from its configured capture input.
Every new occurrence, including generation 1, executes the actual configured
MySQL reader and authoritative admission sealer. It does not import a
`baseline_capture` or `baseline_registry`. A retry of the same immutable
binding/pin returns its saved native request, preventing a second capture.

`connection-capture.py` is the bounded fresh capture/admission recipe step.
It consumes the private connection configuration and emits a receipt with
`full_product_chain_complete: false`; that receipt alone is never a whole
source acceptance. The shared onboarding runner must also execute actual
scheduled Loader/CAS cycles, current collection and S0–S10, maintained Pack,
Gateway/CLI, and current-data dialogue verification before completing.

Fixture SQL clients use an explicit defaults file so a missing unrelated
server include cannot change or prevent the verified reader configuration.
Customer connections remain restricted to approved metadata and LIMIT 5
probes; the fixture full-capture recipe is not a customer authorization.
