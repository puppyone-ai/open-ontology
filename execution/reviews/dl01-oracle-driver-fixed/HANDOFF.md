# Handoff

DL01-W1-42 approves the W34 native-driver preflight at `645d37badf7a56dc26c90dc8cd3b007376489180`. A later Oracle Arrow, PG, Builder, Pack, and non-Pi source-product card can depend on this preflight. This packet is not that card. Source-product count 10 and functional count 8 are unchanged. W05, W06, and the W04 bridge `cdba5f3` were not rewritten.

## Reusable facts

- Probe tree: `testing/dl01-driver-preflight/oracle` at the reviewed commit
- Driver: `oracledb` `=26.0.0-beta.4`, checksum `d729181747bd69ba53cde2956ea9ddc06cf09d0badaf4a9e89944b7c0b7b4397`, feature `arrow`, thin mode, license `UPL-1.0 OR Apache-2.0`
- Arrow crates locked and compiled at `59.3.0` (`arrow-array`, `arrow-ipc`, `arrow-schema`)
- Image: `docker.io/gvenzl/oracle-free@sha256:cdc5b3a3685da4f1785daeee24f74de8a9768b7629192ea9abdaeeea343d12a8`
- Builder: `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0`, host `aarch64-unknown-linux-gnu`
- Banner exercised: `Oracle AI Database 26ai Free Release 23.26.3.0.0 - Develop, Learn, and Run for Free`
- Exact Arrow mapping that decoded: `NUMBER(4)` Int16, `NUMBER(16,6)` Decimal128(16,6), `NUMBER(1)` Int8, DATE `Timestamp(Microsecond,None)`, CHAR and VARCHAR2 Utf8, RAW Binary
- Mappings that stayed uncommitted: unconstrained NUMBER as Float64, `TIMESTAMP(9)` truncated to microseconds, time zone dropped to `None`
- `AS OF SCN` needs a committed row SCN whose `SCN_TO_TIMESTAMP` is after `LAST_DDL_TIME`. This run used 2110487
- Call timeout of 3 seconds ended the polled SQL. The SDK method is `set_call_timeout`. There is no public cancel method in this crate

## How this review ran

`evidence/db8e3f53-43d0-45a4-9866-48edda74f909/DL01/oracle-driver-fixed-review/harness/run-review.sh` extracts the commit, admits under `/tmp/oo-dl01-wave1-resources.lock`, and uses prefix `oo-dl01w1-42`. It mounts the extracted tree read-only and runs the author `build-inside.sh`, SQL, probe, decoder, and `classify.py`.

## Cleanup

Owned containers `oo-dl01w1-42-oracle` and `oo-dl01w1-42-rust` are gone. The private network, oradata volume, secrets under `/tmp/oo-dl01w1-42-secrets`, the extracted tree, and the cargo and target volumes are gone. The two pinned images remain. Foreign containers were not removed. `resource-receipt.json` records `promoted_by_author_gate` true and `secrets_removed` true.

## Next step

A consumer of this preflight still has to bind the same driver digest, Arrow 59.3.0 pins, and banner 23.26.3.0.0 into a full source-product card. That card owns PG, Builder, Pack, and matrix acceptance. No ledger was edited here.
