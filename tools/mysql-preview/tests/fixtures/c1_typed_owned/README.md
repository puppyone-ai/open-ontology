# Owned typed-key C1 slice

This fixture creates **new** MySQL 5.7.44 and PostgreSQL 17 containers. It refuses
existing names/directories and checks loopback ports before creation. It does
not use the C5, Oracle, customer, Gateway, or cloud resources.

```sh
bash product/tools/mysql-preview/tests/fixtures/c1_typed_owned/up.sh
export OO_C1_TYPED_HOME=/tmp/oo-mysql-c1-20261027
export OO_C1_TYPED_EVIDENCE=/absolute/private/evidence/mysql
export CARGO_TARGET_DIR="$PWD/product/target"
cargo test --locked --manifest-path product/tools/mysql-preview/Cargo.toml \
  --test c1_typed_owned -- --ignored --nocapture --test-threads=1
cargo test --locked --manifest-path product/tools/job-worker/Cargo.toml \
  --test c1_typed_ack_live -- --ignored --nocapture
python3 product/tools/mysql-preview/tests/fixtures/c1_typed_owned/verify_audit.py \
  "$OO_C1_TYPED_EVIDENCE"
```

The test reader has SELECT plus replication privileges, verified TLS, ROW/FULL
binlog and GTID. Fixture root performs seed/mutations separately. It tests a
10,000-row composite-key main table, 4,000 inserts / 4,000 updates / 2,000 deletes,
and small string, unsigned-integer and binary-key tables. Independent SQL reads
compare every source/target value without importing the product key/digest code.
Raw audit files and the source read-only statement audit are retained.

The new interface is `mysql_preview::c1_typed_source::Capture`, with
`oo_pipeline::c1_typed::{open, apply}` and
`job_worker::c1_typed_ack::acknowledge`. The explicit 0076 installer requires the
exact 0042 migration. Existing v1/v2 routes and automatic installers are unchanged.

This is **private typed staging**, not the full Loader publication path. It
currently reuses the MySQL v2 lease assertion and contiguous GTID sequence.
Oracle may reuse the key codec/Arrow contract, not the MySQL identity. Source
reconnect/resume, terminal-chunk seals, admission/final publication, independent
control-database CAS, scheduler integration, self-build and business acceptance
remain unimplemented for this opt-in route. The MySQL slice explicitly refuses
float/temporal/JSON columns until its reused assembler is lossless for them.
The key codec permits 8,192 bytes; this B-tree stage refuses identities above
2,000 bytes before COPY.

Containers remain owned by `mysql-c1-20261027` with 1 GiB/2 CPU limits each.
Each run creates a fresh target database; failed databases/spools/logs are kept.
Rerunning the test resets only this fixture's `c1_typed` source tables.
