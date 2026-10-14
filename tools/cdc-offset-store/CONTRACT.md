# cdc-offset-store-v1

Durable control-database mirror of one existing target apply receipt. The mirror is not an acknowledgement. This crate does not certify `source_applied` or `fence_proof`.

Fixed target SQL is `target-bootstrap/1`, `target-fence-receipts/1`, and `target-fence-receipts/3`. `target-fence-receipts/2` is historical: absent is valid, and a stored row must match its checksum. `0004` is not required.

## Calls

`cdc-offset-store install <request.json>`

`cdc-offset-store mirror <request.json>`

One JSON object on stdout. Passwords are files named by the request. Unknown JSON fields are refused.

Exit `0` is `installed`, `replay`, or `mirrored`. A mirrored row with `checkpoint_advanced` false is still exit `0`: the receipt is durable and the completed watermark did not move. Exit `2` is `refused`, including JSON that was read and could not be parsed. Exit `3` is `commit_unknown`. Exit `1` is usage or an unreadable file.

## Connections

Target and control are two databases. Each request names host, port, database, user, password file, CA file, and server name. Host and server name match. On one host the database names differ. TLS is rustls aws-lc-rs, an empty root store plus the caller CA, hostname verification, and no client certificate.

The target session is a least-privileged reader: `SELECT` on `_oo.schema_migrations` and `_oo.apply_receipts` only. The product does not write the target, change its schema, grant privileges, or call `commit_apply_receipt`. The control install user is `oo_cdc_offset_admin` (login, not superuser, no bypassrls, createdb, or createrole). The control mirror user is a login member of `oo_cdc_offset_runtime` (nologin). Runtime grants are `SELECT` on `install_manifest`, `INSERT` and `SELECT` on `mirrored_receipt`, `INSERT` and `SELECT` on `watermark`, and `UPDATE` of `applied_pos`, `applied_txn_id`, `declared_next_txn`, and `declared_next_pos`.

## Target read

Read `schema_migrations`, then the required receipts for this source and transaction. The selected table must be present. Closed receipt rules are checked again in this crate, including config/v1, type-policy/v1, batch/v1, the closed codecs, completion triples, and segment shape. A missing selected row is `target_receipt_absent`. A rule failure on the selected row or a sibling is `receipt_rule` and writes nothing. Migration mismatch is `migration_drift` and writes nothing.

The receipt hash is SHA-256 of the `cdc-offset-receipt/v1` document: one `key=value` line per typed column, including the canonical UTC `target_commit_at`. An existing control row with the same identity and a different hash is `receipt_hash_conflict`. A unique violation on insert is reported as that same fault; this proof is single-writer and does not re-read a concurrent same-hash insert.

## Control decision

In one control transaction, record the verified selected receipt, then compare-and-swap the watermark for that source, codec, lineage, and epoch.

The watermark moves only when every required table receipt is present, none are partial, every segment bitmap is complete, `txn_complete` and `cross_table_consistent` are true, the siblings share codec, lineage, epoch, and position, the caller binding matches that position, `source_gap` and `epoch_barrier` are false, and `expected_previous` is the stored watermark (null only when that epoch has no row). The receipt transaction and position must be the stored `declared_next` when a row exists. The new position must be strictly greater. The new row stores the caller-declared next transaction. Provenance is `caller_declared_pending_real_normalizer`.

Partial segments, incomplete required tables, a source gap, an epoch or lineage barrier, and an expected-previous miss record the receipt and leave the watermark. This crate does not take the maximum position across a hole. A later epoch does not move the older epoch. A second call with `expected_previous` null and `epoch_barrier` false may insert the new epoch.

`checkpoint_advanced` is true only when this transaction's watermark row is the receipt just recorded. `durable_control_mirror` is true only when that receipt row is committed or confirmed by lookup. These stay false:

`source_applied`, `fence_proof`, `ack_allowed`, `external_xack`, `external_xtrim`, `routing_verified`, `real_source_provenance_verified`, `debezium_normalizer`.

`checkpoint_provenance` is always the caller-declared marker.

## Commit uncertainty

A lost control `COMMIT` reply is looked up before another attempt. The same hash is `replay`. Absence retries once on a normal connection. A second loss, or `CDC_OFFSET_RECONCILE=down`, is `commit_unknown` and does not retry. A crash after the target commit and before the mirror is a later mirror of the same target row.

## MariaDB GTID

`mariadb-gtid/v1` stores one domain-server-sequence. `position_lineage` is `d{domain}.s{server}` and `applied_pos` is that sequence, a checked integer of at most 20 digits. Domain is a canonical u32. Server is a canonical u32 other than 0. A comma-separated set is `receipt_rule`. Another domain or server is `lineage_barrier` and is not ordered by sequence. A skipped sequence is `source_gap` unless this receipt is the caller-declared next. The same transaction and sequence is `already_checkpoint`. Another epoch is `epoch_barrier`. `mysql-binlog-pos/v1` is not this codec. `mysql-gtid/v1` is not accepted: same-source UUID GTID set order is not implemented. `ack_allowed` stays false.

## Outside this crate

Source acknowledgement, Valkey `XACK` and `XTRIM`, the reject ledger, large-segment integration, and a path dependency on the moving CDC writer are parent work. The fixture calls `_oo.adopt_target_generation` and `_oo.commit_apply_receipt`. The product only reads the stored receipt.
