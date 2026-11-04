# Committed evidence collector

Run from any directory:

```sh
python3 tools/evidence-index/collect.py
python3 tools/evidence-index/collect.py --check
python3 specs/scripts/gen_support_matrix.py
python3 specs/scripts/gen_support_matrix.py --check --self-test
python3 tools/evidence-index/report.py
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools/evidence-index -v
```

The collector enumerates tracked evidence JSON and engine/oo-test summaries,
then reads **HEAD blobs**, not working-tree edits, untracked files or raw runs.
Commit new source summaries before collecting. The output index, example packs,
contract fixtures and this tool's own files are excluded to prevent self-feeding.
`--report evidence/<run-id>/inventory.json` records every inspected path, its
committed-content hash and emitted row count. Zero-row entries are intentional:
an implementation exit code, aggregate pass count, model usage, cycle count,
image pin, or overall author-complete status is not a named suite verdict.

Adapters cover direct JSON suite records (Oracle, SSH and openGauss), normalized
index rows, PINS and original fixture T0 tables, accepted MySQL T1/T8, historical
MySQL/PostgreSQL component tables, OLAP, MariaDB-family acceptance, SQL Server
validation joined to raw-result hashes, and the S5 explicitly failed T7 probe.
Each adapter has a regression test. Unknown summary formats produce no rows;
add a small adapter and a test when a producer supplies explicit verdicts.

## Admission boundaries

- Exact observed patches are preserved. 23.26.3 is not merged with 23.5, nor
  PG 17 with 17.11. Target IDs match the hand-maintained support matrix.
- Component/fixture-only checks are indexed as `skip`, not product passes.
  Source-oracle comparisons cannot verify Agent DB snapshots; audit smoke
  and nightly aggregate counts do not create product T7 passes.
- Explicit failures and partial/skipped runs remain in the index. No result
  is inferred from a successful command or a missing error.
- A summary-recorded raw suite/result hash is preferred; otherwise the exact
  committed summary bytes are hashed. Image/source/binary hashes are never
  substituted for result hashes. This verifies provenance linkage, not the
  presence or correctness of unavailable raw evidence.
- Emulation comes from recorded execution, or Oracle lane image platforms
  under the documented Apple Silicon execution environment (Plan D1).
- MariaDB-family `3 cycles` cells have no explicit pass verdict and therefore
  do not create T2 rows. Oracle component exits and Builder metrics similarly
  do not create T2/T4/T5 passes. Producer summaries must supply those verdicts.
- The coordinator generator selects the lexicographically maximum `run_id`,
  not a wall-clock timestamp. The collector preserves original IDs and all
  histories; it does not silently reinterpret that shared policy.
- SQL Server `full: verified` is the generator's T0/T1/T7/T8 gate, not full
  product-loop, production, Gateway, or every-version certification.

`REPORT.md` is a generated snapshot of exact levels, every observed P0 patch's
missing T0-T8 suites and untested Oracle 19c. Absence of a row differs from
having only skip/fail evidence. No database is started by these tools.
