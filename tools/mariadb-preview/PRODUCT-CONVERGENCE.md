# MariaDB source convergence (C-S3S4)

The reusable read-only source path now lives in
`crates/oo-connectors/src/mariadb/`, exposed at
`oo_connectors::mysql::mariadb` pending the coordinator's root re-export.
It uses the optional `mysql` feature; C1 additionally requires `mysql-binlog`.

- `MariadbEngine`: explicit MariaDB identity/version rules, catalog discovery,
  source-collation bounded snapshots, file-position watermarks and C5 digests.
- `MariadbChanges`: primary-only ROW/FULL transactions, independent MariaDB
  domain/server/sequence lineage checks, actual retained-log measurement.
- `oo_connectors::mysql::c5::Cycle`: complete range coverage, periodic full
  reconciliation and confirmation only after the consumer accepts replacement.
- Shared MySQL code supplies TLS transport, bounded keysets, budgets and Arrow
  encoding. MariaDB does not use MySQL GTID parsing, authentication defaults,
  binary JSON assumptions or timeout variables. JSON aliases stay LONGTEXT.

Run owned connector fixtures with:

```sh
python3 testing/engines/mariadb/run.py --family mariadb --run-id <unique-id>
```

The old preview remains a historical sealed-import/target-publication harness.
Its fixed fixture catalog, target writes, model decisions and legacy C1 refusal
are not connector code and are not silently redirected. Migrating its CLI and
standalone dependency lock to the product adapter remains follow-up work.
Do not treat this source-level convergence as preview CLI or Agent DB acceptance.
