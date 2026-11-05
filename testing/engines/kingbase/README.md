# KingbaseES V9R1C10 PG mode

S5-E-03, lane KB. Experimental support reuses `PostgresEngine`; it does not
fork the PostgreSQL transport, planner, Arrow reader or C5 implementation.
`kingbase` is an explicit source engine in `oo doctor` and `oo-load`.
The matrix target is `kingbase-pg-mode`.

## Image provenance

`images.lock.json` records the owner-supplied archive SHA-256 and loaded
arm64 image content ID. The archive is never copied into the repository.
The runtime banner is `KingbaseES V009R001C010`; it does not expose B0004.
Build provenance comes from the archive and image identity, not that banner.
The bundled developer license is left unchanged.

Load the owner's archive, check its SHA-256 against the lock, and build:

```sh
shasum -a 256 "$KINGBASE_TAR"
docker load -i "$KINGBASE_TAR"
cargo build -p oo-cli --locked
CARGO_TARGET_DIR=target cargo build --manifest-path tools/oo-load/Cargo.toml --locked
python3 testing/engines/kingbase/run.py --run-id kb-local-acceptance \
  --target-image "$(docker image inspect postgres:17.11-bookworm --format '{{.Id}}')"
```

Both images must already be local. The runner uses at most two owned
containers, labels both `oo.lane=kb` and `oo.run_id`, publishes only random
loopback ports, and removes only its own containers in `finally`.
Only the Kingbase container uses `--privileged` and `/usr/sbin/init`.
It changes neither binfmt nor host sysctls. Credentials and TLS private keys
are generated into ignored `evidence/<run-id>/private/`, never argv.
An administrator configures TLS, creates fixtures and grants read access;
product connections use a separate reader account with verified server TLS.

## Acceptance boundary

- T0: real product doctor, PG-mode product identity, plans, estimates,
  unsupported C1, and actionable missing-SELECT advice. The model probe is
  a local synthetic replay, not live model acceptance.
- T1: a 2,000,000-row integer-key table, text key (including empty and Unicode
  values), composite key, unique-only key, no-key table, growth table and
  a 300-column table. A concurrent administrator inserts, updates, deletes
  and changes primary keys during the snapshot. Comparison occurs after
  exactly one C5 correction cycle, with the writer stopped.
- C5: three cycles; the second is idle and must produce zero payload,
  zero target tuple writes and zero new generations; the third grows a
  table. Independent source/Agent DB row counts and canonical SHA-256
  chunks (10,000 rows each) must match for every table after every cycle.
- T8: persistent admin sampling of reader sessions, peak <= 2 and sampled
  aggregate duty <= 25%; product receipts retain target logical tuple bytes.
  No model is invoked by the loader. These are sampled connection/activity
  measurements, not physical WAL, disk or network accounting.
- C1: the supplied image rejects `pg_create_logical_replication_slot` with
  `pgoutput` because that library is absent. Its bundled
  `syslogical_output`/`decoderbufs` plugins are different protocols, not
  evidence of compatibility with the existing PostgreSQL C1 reader.
  Doctor reports unsupported and load refuses C1; it is never a passing row.

The product version is reported as 9.1.10, independently of the server's
PostgreSQL compatibility version 12.1. Other releases, Oracle/MySQL modes,
and PostgreSQL servers masquerading as this family fail closed.
The extra reserved keyword `enckey` is isolated to verified Kingbase
sessions; PostgreSQL identifier mapping stays unchanged.

This is a local Agent DB import, not Gateway publication. It does not cover
hundreds of tables, all common-fixture types/collations, T3-T7, a live model,
or production licensing. No support-matrix grade is changed automatically.
The coordinator owns ingestion of `index-rows.json` and broader acceptance.

```sh
python3 -m unittest discover -s testing/engines/kingbase -p 'test_*.py'
PYTHONPATH=tools/oo-test python3 -m unittest test_matrix test_verdicts
python3 testing/engines/kingbase/recompare.py --run-id kb-local-acceptance
```

`recompare.py` independently rechecks retained row outputs with the shared
typed oracle and refuses to overwrite an existing comparison. See
`evidence-summary.json` for measured verdicts, raw-evidence hashes, the
preserved failed attempts, and acceptance gaps.
