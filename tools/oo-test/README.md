# oo-test: owned synthetic fixtures

Python 3 standard library + Docker CLI. No root Cargo dependency/lock changes.
Run from this checkout; the tool resolves paths relative to its own file.

```sh
tools/oo-test/oo-test matrix --engine mysql --versions 5.7.44,8.0,8.4,9.x --suites T0
tools/oo-test/oo-test matrix --engine postgres --versions 13,14,15,16,17,18 --suites T0
tools/oo-test/oo-test matrix --engine oracle --versions 23ai,21c,11g --suites T0
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools/oo-test -v
```

Default seed: 2,000,000 events, 200 small tables, 300-column wide table.
`--scale 1000 --tables 3` is a **reduced smoke test**, not scale acceptance.
Versions resolve to the newest matching **locked** patch, never a registry tag.
PostgreSQL 12 is selectable but experimental. Oracle `23ai` resolves to 23.5,
not the later 26ai releases that also start with 23.

The runner executes one database at a time, publishes no host ports, creates
an explicitly labelled data volume, and uses platform manifest digests.
Missing native architecture selects `linux/amd64`; resource receipts say
`emulated` when this differs from the Docker server architecture.
`--platform linux/amd64` overrides selection. Native readiness timeout is
300 seconds, Oracle/emulated timeout 1200 seconds (`--health-timeout` override).
Registry pulls retry at most three times; each failed attempt remains evidence.
`--runner remote` fails explicitly without attempting SSH.

## Evidence and acceptance boundaries

Each invocation prints `evidence/h-s0-<uuid>/`. Each version directory contains:

- Numbered argv/exit records and stdout/stderr (passwords and signed URL query
  strings redacted; SQL containing passwords travels through stdin only).
- `seed.json`, `fixture-shapes.json`, `gold-answers.json`.
- `comparison-report.json`, `resource-receipt.json`, `SHA256SUMS`.
- A private temporary directory, deleted during cleanup; never commit it.

Use `--baseline evidence/<other-run>/<engine-version>/gold-answers.json` to
compare all 20 source-computed answers across engines. Without a baseline,
the first successful version is **reference-only**, not independent correctness
proof. Subsequent versions must match; mismatch is a nonzero exit. Preserve
failed runs and use a fresh run id for retries.

T0 here is the card's fixture precheck: boot, admin seed, separate read-only
query principal, table counts, 300-column check, exact version and gold.
It is **not** product `oo doctor`, permission diagnostics, T1-T8, an Arrow
round-trip, or product acceptance. Consistency is explicitly `B` (quiescent
owned fixture; no claim of transactional snapshot across separate clients).
Final Docker stats are a sample, not measured peak/duty-cycle proof; unknown
metrics are null. A nonzero command or unimplemented requested suite fails.

Cleanup checks both `oo.run_id` and `oo.lane=h` before removing named resources.
No prune, image deletion, anonymous volume deletion or unrelated-container
operations. SIGINT/SIGTERM attempt cleanup; SIGKILL/host failure cannot, so use
the receipt's exact names/labels for manual recovery. Pulled images stay cached.

## Engine-lane hooks

```sh
tools/oo-test/oo-test matrix --engine postgres --versions 17 \
  --suites T0-T2 --hook T1=./my-t1 --hook T2=./my-t2
```

Hooks are executables, invoked without shell evaluation after seeding. They
must return nonzero on any failure. `OO_TEST_CONTEXT` points to a JSON file
with `container`, `engine`, `version`, `run_id`, `evidence`, `reader`, and
`reader_password_file`. The container has no exposed host port. Use `docker
exec` or a lane-owned client on its network. Do not put passwords in argv.
The MySQL reader defaults file inside the container is `/run/oo/reader.cnf`;
PostgreSQL reader uses the local socket; Oracle reader uses the password file
via SQL*Plus stdin and the FREEPDB1/XEPDB1 service (11g uses XE).
Only the separate fixture admin may apply growth/change workloads. Product
connections must always use the read-only reader. Hooks own their additional
labelled resources and evidence; a zero hook exit is not automatic capability
admission. Missing T1-T8 hooks are `not_implemented`, never skipped-as-passed.

## Comparison API

Import `comparison.py` by adding `tools/oo-test` to your Python module path.

```python
from decimal import Decimal
from comparison import compare

report = compare(
    [(1, Decimal("2.0000")), (2, None)],
    [(1, Decimal("2")), (2, None)],
    key_columns=[0], chunk_size=10000, consistency="S1",
)
assert report["equal"]
```

`compare(left, right, *, key_columns, chunk_size=10000, consistency="B",
oracle_empty=False)` consumes two iterables once, bounded to one row per side.
The caller MUST supply rows already ordered **by the source's selected key
and collation**, from the appropriate snapshot. Never Python-sort varchar
keys. Composite keys use multiple indexes. No-key callers must supply a
stable source-defined identity/order or decline comparison.

Output includes ordered per-chunk row counts and SHA-256 hashes, the first
differing row offset and both keys (one null for a missing tail), the chosen
consistency level, and explicit Oracle empty-string policy. Missing/extra rows,
changed keys, order changes and value differences fail equality.

`canonical(value, oracle_empty=False)` supports Python int/Decimal (lossless
decimal text, insignificant zeroes removed), float (IEEE-754 binary64 bits,
including signed zero/NaN payload), str, bool, None, bytes, date, datetime,
lists and string-keyed dicts. Aware timestamps retain UTC microseconds plus
original offset and IANA zone when available; naive timestamps are distinctly
tagged local timestamps, never silently assigned UTC. Pass Decimal, not
float, for exact database numerics. Text is not case/accent/space-normalized:
key collation controls ordering, not stored-value equality. Decode JSON into
dict/list for semantic object-key normalization; use str for textual equality.
`oracle_empty=True` collapses SQL-cell empty text to NULL explicitly, never
empty strings nested inside JSON objects/arrays; source provenance must be
retained by the calling lane.

The hash format is `oo-test-typed-json-v1`: UTF-8 canonical typed JSON per row,
compact separators, sorted object keys, plus one LF. It is intentionally
**not** `ChunkReceipt.sha256` (Arrow IPC) or `RangeDigest.digest`
(order-independent C5). Do not compare these hash domains directly.
`TableRef`, watermarks and plan revision remain engine-owned metadata; attach
this oracle's report to them without changing their serialization.
## Lane M incremental T2

```sh
tools/oo-test/oo-test t2 --versions 13,14,15,16,17,18 --run-id pg-t2-local
```

This entry owns one labelled PostgreSQL container at a time, with distinct
source/Agent databases and separate reader/admin connections. It runs the real
`oo-load` binary against a compact two-table fixture: three C1 and three C5
cycles, concurrent admin writes, exact row comparison after each cycle,
monotonic durable C1 positions, T8 sampling and a no-change cycle with zero
logical target writes/new generations and unchanged total target relation size.
C5 additionally requires zero delivered payload on its no-change cycle.

It retains concurrent-range retry failures and requires a successful repaired
cycle; it does not mask unexpected source failures. Negative checks recreate a
replication slot and a source table, requiring explicit rebaseline errors.

Raw per-version commands, receipts, comparison hashes, resource/image receipts,
T8 samples, failures and cleanup results live under `evidence/<run-id>/`.
The command exits nonzero for missing/failed T2 or cleanup failures.

This does not exercise C5-to-C1 handoff, full type/scale fixtures or T7. MySQL
and Oracle T2 are not implemented by this entry; do not infer those engine
exits from the PostgreSQL results or the separate synthetic-source target tests.
