# E-S5c: typed Oracle SourceEngine

## Results

Card status: **partial**, pending the shared negative-scale loader changes.
No whole-plan TODO row is marked complete here.
`typed-evidence-summary.json` records raw-result SHA-256 references.
All final accepted T1 runs use oo-load SHA-256
`10f3925f6802ac0b2f487d7c12c817920a43da203a11e5e00bda630501b30b31`.

| Version | Full T1/T8 | T2 C5 | T2 C1 | T1 run-id |
|---|---|---|---|---|
| 23.5.0.24.07 | passed; 231 tables, 2M events | 3 cycles passed | 3 explicit opt-in cycles passed | `e-s5c-t1-final23-20261105` |
| 21.3.0.0.0 | passed; 231 tables, 2M events | 3 cycles passed | not run | `e-s5c-t1-utc-20261105` |
| 11.2.0.2.0 | passed; 231 tables, 2M events | 3 cycles passed | not run | `e-s5c-t1-utc-20261105` |

T2 evidence: `e-s5c-t2-20261105`; empty-cycle assertions pass.
Gateway evidence: `e-s5c-gateway-20261105`. Authenticated mTLS queries return
HTTP 200 with a publication ID. SUM/MIN/MAX are
`7275.000 / 1.125 / 120.125`, numerically equal to source gold
`7275 / 1.125 / 120.125`; all 120 ORDER BY rows equal the source order.
This is a real loopback Gateway over an owned fixture publication, not a
public/customer deployment or reviewed Self-build acceptance.

`e-s5c-typed-20261105` records workspace check, the complete crate test run,
base-failure reproduction, and standalone Python tests. Library tests:
163 passed, one live test ignored. Python scripts: 2/3/12 passed. Spec,
plan and v0.2-plan validators passed. All created containers were removed;
no owned Gateway process remains.

## Contract

The SourceEngine now carries `ALL_TAB_COLS.DATA_PRECISION/DATA_SCALE` into
Arrow. Snapshot pages and admitted LogMiner row images use one converter.
NUMBER with scale zero and precision at most 18 uses int64; other representable
bounded NUMBER uses decimal128. Unconstrained NUMBER (and decimal declarations
not representable by this Arrow decimal contract) uses lossless UTF-8 marked
`oo.oracle.mapping=DecimalUtf8` and `oo.oracle.decimal=exact`. It intentionally
loads into PostgreSQL text; this does not make SUM(text) admissible.

DATE/TIMESTAMP use naive microseconds. Zoned timestamps, including LOCAL TIME
ZONE, project explicit UTC instants; original offset/region spelling is not
retained. Nonzero submicroseconds still fail closed, as in `arrow_part`.
RAW/BLOB use binary; CLOB/NCLOB use UTF-8, preserving empty LOB versus NULL.
BINARY_FLOAT/DOUBLE use big-endian IEEE source bytes, not decimal text.
BINARY_FLOAT is widened exactly to float64.

C5 content hashes frame typed values; source range digests are namespaced
`typed-v1`. IPC receipts also change. **Rebaseline old Oracle generations,
chunk receipts, C5 saved ranges and checkpoints; do not resume old all-text
generations with the new binary.** The source SQL digests remain change hints,
not cryptographic proofs of Arrow content. LogMiner's existing NUMBER/text
admission restriction is unchanged; this card does not broaden C1 admission.

## Reproduction

Raw outputs are in ignored `evidence/`, not in this commit.

```sh
cargo fmt -p oo-connectors
cargo check --workspace --locked
cargo test -p oo-connectors --features oracle --locked --no-fail-fast
CARGO_TARGET_DIR="$PWD/target" cargo build --manifest-path tools/oo-load/Cargo.toml --locked
python3 testing/engines/oracle/test_typed_acceptance.py
python3 testing/engines/oracle/test_t1_hook.py
python3 testing/engines/oracle/test_audit.py
python3 testing/engines/oracle/typed_acceptance.py t1 --engine oracle \
  --versions 23,21,11 --run-id e-s5c-t1-utc-20261105 \
  --worker-image oo-e-oracle-worker:e-fix-worker-v4
python3 testing/engines/oracle/typed_acceptance.py t2 --engine oracle \
  --versions 23,21,11 --run-id e-s5c-t2-20261105 \
  --worker-image oo-e-oracle-worker:e-fix-worker-v4
python3 testing/engines/oracle/typed_gateway.py --run-id e-s5c-gateway-20261105
python3 testing/engines/oracle/typed_acceptance.py t1 --engine oracle \
  --versions 23 --run-id e-s5c-t1-final23-20261105
```

Builds/tests used `CARGO_PROFILE_DEV_DEBUG=0` and
`CARGO_PROFILE_TEST_DEBUG=0` to bound local disk usage. The two source/target
database containers run sequentially by version. The optional 11g worker
is a client-only container, not a third database. Every created container is
labelled `oo.lane=e`. The Gateway token exists only in process memory, not a
token file. Its fixture publisher is **not** reviewed Self-build or the real
20-question model dialogue.

T1 additionally checks every target column's actual PostgreSQL type before
comparing values. It parses JSON decimals without floating-point conversion,
decodes bytea framing, and compares zoned timestamps as UTC instants.
It also runs direct Agent DB SUM/MIN/MAX/ORDER BY on the orders amount column;
this direct check alone is not Gateway evidence.

## Failures retained

The first T1 attempt (`e-s5c-t1-20261105`) failed comparison on TYPE_EDGES for
23/21: the old comparison included the original timezone offset in its hash.
Target schema checks already passed. The UTC-instant adapter fixes the
comparison contract, not source data or gold values. The 11g run was
interrupted before acceptance so the corrected three-version run could start.

`parse_file_cli::parent_deadline_kills_the_worker_and_the_next_call_completes`
fails on this macOS host: process-group signal `rejected` versus expected
`sent`. The identical assertion was reproduced on an untouched `git archive`
of the rebased base, inside ignored evidence. It is not repaired or skipped.
The other crate targets ran with `--no-fail-fast`; library tests pass.

Combined Python discovery encounters the existing two modules named `audit`.
The three test scripts run in separate processes instead; no test is removed.
The original `test_t1_hook` discovery import failure was also reproduced on
the unchanged base archive.

## Coordinator-owned changes

`E-S5c-shared.patch` contains exact, unapplied proposed changes:

- The shared PostgreSQL loader currently rejects negative decimal scales.
  Oracle produces exact decimal128(p,negative-scale), but these cannot reach
  Agent DB until the loader and COPY renderer are updated together. The patch
  uses unconstrained PostgreSQL numeric and exact coefficient expansion,
  without rounding or a Gateway cast. Add loader/COPY boundary tests before
  integration; only `git apply --check` was run here.
- Two legacy ignored component fixtures outside this card's write scope
  downcast numeric IDs to StringArray. The patch adapts those IDs to int64.
  The real oo-load T1/T2 runners do not use these old sinks.

No root Cargo manifest, Cargo.lock, shared engine, migration, docs/specs,
execution record, support matrix or Gateway cast fence is changed.

Full S5-E-04 T3/T4 and full S5-G-01 all-P0 model dialogue remain coordinator
acceptance gates; this narrow card must not mark either whole-plan row done.
