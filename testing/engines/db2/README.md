# Db2 LUW worker (S5-E-03)

The `db2` connector feature introduces no Rust dependency or native link.
The source-only worker uses IBM `ibm_db` 3.2.6 / CLI and Arrow 22.0.0 in a
separate local image. Never publish that image: the native driver remains
customer-provided. Cargo deny/audit covers Rust, not the IBM binaries.

Driver decision, 2026-11-05: the public IBM Rust driver's README requires
unixODBC and IBM CLI. The current search found no admissible pure-Rust DRDA
candidate; this is not a proof that no such implementation exists.
Originals retained in `evidence/e-s5f-20261105/`:

- `https://github.com/ibmdb/rust-ibm_db`
- `https://github.com/ibmdb/python-ibmdb`

Build the worker with `docker build --platform linux/amd64 --label oo.lane=e
-t LOCAL_TAG testing/engines/db2/worker`. Resolve the built digest and use a
trusted wrapper executable that runs that exact image with `--rm -i`,
`oo.lane=e`, an owned run label and bounded CPU/memory. The wrapper must not
receive Agent DB credentials. Source credentials travel only on stdin.
`oo-load --db2-worker WRAPPER` supports snapshot publication and `--c5 FILE`. Remote
connections require a caller-provided CA mounted read-only inside the worker
at `/run/oo-source/ca.pem`; plaintext is restricted to literal loopback.
SSH and log CDC are not integrated. C5 uses the shared durable range loader,
binds the table incarnation and full column/key catalog to its lineage, and
retains the baseline key ranges when live row boundaries move.

The connector plans primary/composite keys using source-ordered probes,
never a client collation. No-key tables use a single bounded read. Plans are
registered and stale/forged chunks are refused. Concurrent growth beyond
the row/byte/deadline budget fails before any sink publication; the shared
loader may replan. Metadata and data calls are paced. Empty tables emit an
empty schema-bearing Arrow batch. C5 hashes are order-independent and
include duplicate multiplicity and nulls. Watermarks are observation times,
not log positions; consistency is B, not a global snapshot.

Current admitted scalar types: SMALLINT, INTEGER, BIGINT (Arrow Int64);
CHAR, VARCHAR, DECIMAL, DATE, TIME, TIMESTAMP (lossless Arrow text).
Decimal and temporal native target typing is **not** claimed. Unknown
types, LOBs and unsupported key types fail closed. Db2 LUW 11.5 only;
12.x, z/OS and IBM i are not admitted by this implementation.

Unit checks:

```sh
cargo test -p oo-connectors --features db2 --lib --locked
python3 -m unittest discover -s testing/engines/db2 -v
```

Real fixture sequence (a new run-id each time):

```sh
python3 testing/engines/db2/preflight.py
python3 testing/engines/db2/fixture.py --run-id RUN
python3 testing/engines/db2/seed.py --run-id RUN --full
# Create evidence/RUN/worker using the pinned wrapper contract above.
cargo build --manifest-path tools/oo-load/Cargo.toml --locked
python3 testing/engines/db2/target_acceptance.py --run-id RUN --full
```

The full seed is 231 tables, 2M events and 300 columns in the wide table.
The separate admin writer performs insert/update/delete/primary-key moves.
The harness quiesces that writer, rereads the full B-consistency cut, compares
every source/Agent DB row, then checks no-rewrite resume. The target's psql
uses `FETCH_COUNT=1000` so its comparison oracle does not buffer 2M rows in
the 512 MiB target container. All failures and retries have separate paths.
The caller must clean up only its recorded source container in a `finally`
block; the target runner cleans up its own target even on failure.

The coordinator registered Db2 in `oo doctor` and the shared target mapper.
`product_t0.py --run-id RUN --binary target/debug/oo` runs the real doctor
with H's `tools/oo-test/t0.py` validator and a local model replay. It does not
promote connector probes to product T0. The current doctor pre-dispatch
allowlist still excludes Db2; E-S5g records this as a failed T0, with an
exact coordinator patch in its summary.

`target_acceptance.py --run-id RUN --attempt c5 --c5` adds three real
`oo-load --c5` cycles: digest baseline, changed range, idle cycle. Each
compares every row with Agent DB; idle cycles must have zero target writes
and new generations, and the last must read no payload. Without `--full`,
this is a focused four-table fixture, not full-scale matrix acceptance.

The fixture admin uses a separate local instance-owner connection. Reader
credentials are generated into ignored mode-0600 files. Delete only the
container recorded in this run's `fixture-owned.json` after testing.
