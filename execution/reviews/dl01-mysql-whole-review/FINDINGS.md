# Findings

Judgment: the experimental child exits are green. The items below are the gaps this review was asked to settle. None of them is an open original gate.

## Historical compiler file and the current seal

`scale/phases/proof.exit` is the bytes `1\n`. `scale/phases/blocker.txt` is `compiler refusal missing`. The document beside them, `scale/phases/import-seal.json`, is a real `compiler_refused` result from `compile_captured_pg_catalog`, code `oo.builder.catalog-evidence/v1/field.unknown`, message `catalog evidence is not oo.source.pgsql/v1`, model `not_called`, pack `not_produced`. That is a preserved failed attempt whose harness looked for a spaced field and missed the compact JSON.

The current native seal is a different document. `native-loop/raw/import-seal.json` is `draft_compiled` from `compile_sealed_import_catalog` at dependency `b1ffb079`, identity `e6c65a3c25b0eac2c5c8615e94ddec88e158adffdcbbed89fcb00ac23a270af6`, on the live sealed run. The same compiler refuses a bad catalog in `raw/attempts/import-seal-chunk-batches-import-seal.json`: `compiler_refused`, `quality.rejected`, `a stored batch does not match the sealed table`. `whole-source-delivery.json` has `remaining_conditions` empty. That empty list matches the current compiler. The old proof file stays in the packet.

## Identity is the sealed physical import

`e6c65a3c25b0eac2c5c8615e94ddec88e158adffdcbbed89fcb00ac23a270af6` recomputes from the live run `b512537d-ea3c-4eda-8bba-3eef0afe52ec`: engine `mysql`, source `mysqlscale`, the four digests above, generations 1 and 4. The operator replay binds that same import and snapshot. Rows live in hashed `_oo.pt_*` tables. An open mysqlscale run `e8343223` at target generation 3 is unsealed, with hubs applied 2000 and the other four tables applied 0. It is not the consumer snapshot. The sealed generation 4 tables are all exhausted.

## Ready false, 43 columns, unit XXX

Fresh and cache both say `ready` false, publication `not_published`, review `unreviewed`, and they carry the same pack. The operator queries that unpublished pack and returns the sealed values. Publication is a later DL boundary.

The completion has 43 columns. Two money units are the literal `XXX`. The binder stored them. `effective.json` still has `XXX`. The unit string `unknown` is absent. This is an unreviewed draft currency label, and it is the label the file contains.

## Declared price and unknown cost

These are two configurations. Live config `9c217c2f…` declares a non-billing USD price. Cost config `000c7d15…` declares price status `unknown`. The model bodies are byte-identical. The unknown-price result stops at `cost_unknown` / `unknown_cost` with no pack. The declared-price result is the unpublished pack. The refusal follows the price configuration.

## Input-hash miss, 401, timeout, 402, 503

The miss result is HTTP 401, pack `not_produced`, and its recorded input sha256 differs from the fresh result. The miss `model-input.json` file is the same 8342 bytes as the fresh prompt. The hash change is the builder input, which includes the visibility file the author script swapped in. It is that 401 gate. HTTP 401, timeout, 402, and 503 each leave `ready` false and pack `not_produced`. The 266-hash check covers those files. They were not overwritten.

## Source TCP 124 and the following queries

The author script fails the detach phase when the MySQL bridge address connects or the target port does not. The passing log line is `mysql_tcp=124 pg_tcp=0 dns_tcp=0 mysql_ip=192.168.160.2`, and the consumer phase follows it. This review saw the same split from inside the Postgres container. The DNS address `198.18.2.127` accepts TCP and returns an empty read, with no protocol byte and no version string. That open port is not a MySQL greeting from 8.4.10. The source container is still running on `192.168.160.2`, which the Postgres network does not reach. The replayed queries record `source_dialed` false. The earlier `mysql_tcp=0` failure remains in `inside.log` as `NATIVE_EXIT 1`.

## Hidden column and the database role

Operator `SELECT comment` exits 6, class `guard`. A policy copy that adds `comment` makes describe exit 9, class `pack_mismatch`. Those are the pack exits.

`oo_preview_login` cannot select the physical shipments table. `SET LOCAL ROLE oo_r_reader` can. In that role, `is_superuser` is off, `has_table_privilege(..., 'SELECT')` is false, and `count` of the physical comment column is 7814. The table ACL does not grant that role. The operator guard holds on the logical name. Direct SQL as `oo_r_reader` can see the physical column. That direct path is outside the operator process. It does not reopen the operator exit, and it is not a database column grant this review can point to in `relacl`.

## Types, empty, cancel, and half-ready

Unsigned max, scale-4 money, nulls, `bytea`, `json`, date, and time were read from MySQL and from the sealed physical columns. Rejected zero-date and time-range values are null in the stored probe and present as three range-gap rows. Empty `empty_box` on the sealed prototype run is 0. The in-flight cancel returned sqlstate `57014` on both the cancel call and the query, with `rollback_confirmed` true and 0 returned rows. The sealed run stays `awaiting_publish` with every table exhausted. The open generation-3 run stays unsealed and incomplete.

## Secrets and tests

Password files of 12 bytes or more do not appear in this review's raw outputs or in the author native-loop tree. The model token file is 6 bytes, so a substring scan would not be meaningful, and its bytes were not printed. Author unit tests were not treated as the product exits. The exits above are the live source, the live sealed tables, the frozen result files, and a new operator process.
