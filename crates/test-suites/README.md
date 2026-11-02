# Reproducible crate tests and explicit fixture suites

## Default product suite

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
python3 -m unittest discover -s crates/test-suites -p 'test_*.py'
```

The default suite needs the pinned Rust toolchain, the resolved Cargo dependencies,
libclang and protoc (for pg_query), and ordinary local process/loopback access. It
does not need a source database, customer credentials, an `evidence/` checkout, or
`OO_*_FIXTURE` pointers. CLI tests build their cross-package producer binaries with
Cargo, rather than relying on historical binary paths. The three reviewed-policy
suites still accept explicit `OO_A227_*`, `OO_A234_*`, and `OO_A254_*` binary overrides
for controlled replay.

Native Linux is the complete process-supervision test platform. Twelve tests that
inspect `/proc` process identity are explicitly platform-ignored on non-Linux;
they remain in the **default Linux suite**. Do not use amd64 emulation on arm64
for the process-identity evidence: `/proc/<pid>/exe` can identify the emulator.
Use a native-architecture Linux runner with libclang and protobuf-compiler.

## Discover and select a fixture (never an implicit all-fixtures run)

```sh
python3 crates/test-suites/fixtures.py list
python3 crates/test-suites/fixtures.py list --suite live
python3 crates/test-suites/fixtures.py list --suite evidence
python3 crates/test-suites/fixtures.py list --suite platform
python3 crates/test-suites/fixtures.py list --suite artifact
python3 crates/test-suites/fixtures.py run oo-connectors live_source_catalog
```

The inventory is derived from the Rust test attributes, and prints the package,
test name, source path, suite, and reason. The runner builds the selected package,
checks the compiled ignored-test inventory, requires exactly one matching test,
and executes it with `--ignored --exact --test-threads=1`. An unknown name, an
uncompiled feature, missing inputs, a bad pointer, or a failed test is nonzero.
Use a fully qualified test name if needed. Optional connector fixtures can add
`--features mysql` or `--features oracle`. Listing does not build or connect.

`live` means an owned database/service fixture; `evidence` means a retained
capture/validator artifact replay; `platform` means OS-specific process checks;
`artifact` means explicit fixture generation. Historical artifact replay is not
live acceptance. Default tests that generate synthetic data are not in any of
these opt-in suites.

Do **not** run `cargo test --workspace -- --ignored` against a machine with
customer services. Fixtures can create/drop databases and roles. The runner does
not create infrastructure or borrow existing containers. Provision a fresh,
isolated fixture and inspect the indicated source's pointer struct, environment
variables and assertions before selecting it. Fixed hostnames and `/run/...`
credential paths belong inside that fixture's private test network/container.

### PostgreSQL connector fixtures

Each pointer is a JSON file with the exact fields declared by `Pointer`, `Fixture`,
or `load_pointer` in its source. It references fixture-only credentials, TLS CA,
roles, databases and logs; just providing a PostgreSQL port is not sufficient.
The tests retain their pinned version, privilege, TLS and counter assertions.

| Test target | Required pointer/environment |
| --- | --- |
| `live_pg` | `OO_PG_FIXTURE` |
| `snapshot_live` | `OO_PG_SNAPSHOT_FIXTURE` |
| `lossless_live` | `OO_PG_TYPES_FIXTURE` |
| `catalog_rate_live` | `OO_PG_CATALOG_RATE_FIXTURE` |
| `pg_portal_live` | `OO_A127_FIXTURE` |
| `pg_reader_cancel_live` | `OO_PG_CANCEL_FIXTURE` |
| `pg_reader_complete_live` | `OO_PG_COMPLETE_FIXTURE` |
| `pg_reader_drop_live` | `OO_PG_DROP_FIXTURE` |
| `pg_reader_finish_live` | `OO_PG_FINISH_FIXTURE` |
| `pg_reader_keys_live` | `OO_PG_KEYS_FIXTURE` |
| `pg_stream_live`, `pg_stream_bounds`, `pg_stream_acl_live` | `OO_PG_STREAM_FIXTURE` |
| `pg_window_live` | `OO_PG_WINDOW_FIXTURE`, evidence/output and budget/admission binary paths declared in the module |
| `pg_wire_tls_tail::tls_pg_sixty_thousand_small_rows` | `OO_A133_TLS_FIXTURE` |

For example, after provisioning the selected owned fixture:

```sh
OO_PG_FIXTURE=/absolute/private/fixture.json \
  cargo test --locked -p oo-connectors --test live_pg \
  live_source_catalog -- --ignored --exact --nocapture --test-threads=1
OO_PG_FIXTURE=/absolute/private/fixture.json \
  python3 crates/test-suites/fixtures.py run oo-connectors live_source_catalog
```

If `OO_PG_FIXTURE` is absent, the selected test **fails**, not passes or skips.
Legacy gateway `OO_A*_LIVE=1` flags now accompany explicit ignored tests; an
absent flag fails when selected. They still need the private TLS role-pack and
module-specific fixture paths, not just the flag.

### Owned pipeline/library fixtures

`native_c5_pg17` and `native_c5_merge_pg17` need `OO_C5_PG_PORT` pointing to a
**fresh disposable PG17 cluster**, localhost, with the test-only PostgreSQL
password specified in their `connect` functions. Run each on a separate cluster;
the lifecycle test drops its named database and roles. Library tests need
`OO_LIBRARY_PG_PORT` and, where declared, `OO_LIBRARY_FIXTURE`,
`OO_LIBRARY_BUILDER_INVENTORY`, `OO_LIBRARY_BUILDER_EXPORT`, or
`OO_LIBRARY_GATEWAY_INPUT`. These are owned integration fixtures, not ordinary
builder/policy unit tests.

```sh
OO_C5_PG_PORT="$OWNED_FRESH_PG17_PORT" \
  python3 crates/test-suites/fixtures.py run oo-pipeline pg17_c5_lifecycle
OO_C5_PG_PORT="$OTHER_OWNED_FRESH_PG17_PORT" \
  python3 crates/test-suites/fixtures.py run oo-pipeline merged_migrations_fresh_and_upgrade
```

### Retained evidence

Tests explicitly named historical/retained/frozen compare pinned bytes or execute
an approved frozen validator. Synthetic data must not be represented as that
historical proof. Restore the exact relative `evidence/...` paths declared in the
listed source. Native-arrow admission replay accepts `OO_NATIVE_RECEIPT`,
`OO_NATIVE_DECISION`, and `OO_NATIVE_ADMISSION_BIN_PATH` overrides; other capture
paths remain the exact source-declared paths. The frozen validator requires its
native executable platform and approved digest. Selecting the replay no longer
silently passes when files or an executable are missing.

```sh
cargo test --locked -p oo-pipeline --lib \
  native_binding::tests::historical_receipt_is_clickhouse_or_opensearch_only \
  -- --ignored --exact --nocapture
python3 crates/test-suites/fixtures.py run oo-pipeline \
  clickhouse_binding_plans_when_the_frozen_cli_admits
```

## Deterministic synthetic coverage

- `oo-core/tests/fixtures/source-bound/` contains synthetic Oracle catalog and
  dictionary pairs. Source-binding cross-source/run/digest/column/token checks
  and semantic derivation remain default tests. Regenerate with Python stdlib:
  `python3 crates/oo-core/tests/fixtures/source-bound/generate.py`.
- Native-update key-model tests generate Arrow in-process, checking duplicate
  key conflicts, unique/aggregate/snapshot selection and required partition or
  upsert windows. Oracle wide-number semantic derivation uses the same synthetic
  catalog, not an optional historical path.
- Native-arrow's synthetic registry/receipt exercises the actual binding guard;
  it is not a new approved validator or a change to admission policy.
- `synthetic-standard.arrow` and `synthetic-nested.arrow` exercise decimals,
  date/time units, nullable fields, fixed binary, uint8, lists, structs and dense
  unions without retained source captures. Their generator is
  `crates/oo-pipeline/tests/fixtures/native/generate_synthetic.py`
  (Python 3.12 with `pyarrow==21.0.0`). Cargo reads the committed files and does
  not require Python or PyArrow. Pinned historical decoder checks remain in the
  evidence suite, alongside these default synthetic checks.
