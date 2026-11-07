# M-S1c sign-off: partial

Rebased before editing; branch was already current at `0489031e6`
(including the card's shared-interface fixes).
Root Cargo manifests/lock, shared interface, engine code and existing migrations
are unchanged by these commits. A separate `tools/oo-load/Cargo.lock` is added.
No external source, customer environment, other worktree or service was changed.

## Delivered and measured

- Generic plan/read/IPC admission, revision coverage, bounded chunks, receipt
  persistence, Oracle empty-string semantics, target typed COPY, local atomic
  view replacement, replay and write metering.
- `oo-load` stack JSON/YAML entry with secret references and PostgreSQL, Oracle
  and MySQL constructors. PostgreSQL physical reads hold an exported read-only
  snapshot. MySQL's existing empty-batch implementation already satisfies the
  contract; it was not modified. Real MySQL matrix was not run.
- T1 hook retains H's API, seeds all H fixtures and runs insert/update/delete/
  primary-key mutations through a separate admin connection. Correction is a
  full reread after the writer quiesces, followed by typed ordered comparison.
- Final target regression includes fake PostgreSQL and MySQL-family engines,
  exact unsigned maximum, empty replacement, superseded-run rejection, and
  two real SIGKILLs. After stage kill, publication kill and resume: one receipt,
  three rows, 174 logical write bytes; exits -9, -9, 0.

## Real matrix

Default scale: 2,000,000 events, 200 small tables, 300-column table, all H shapes.
PostgreSQL has 233 tables per version. Comparison consistency is **B**.

| Engine version | T1 | Logical target bytes | Evidence run |
|---|---|---:|---|
| PostgreSQL 13.23 | pass | 223627432 | m-s1c-pg-full |
| PostgreSQL 14.24 | pass | 223595599 | m-s1c-pg-full |
| PostgreSQL 15.19 | pass | 223577965 | m-s1c-pg-full |
| PostgreSQL 16.15 | pass | 223591834 | m-s1c-pg-full |
| PostgreSQL 17.11 | pass | 223571719 | m-s1c-pg-full |
| PostgreSQL 18.6 | pass | 223447639 | m-s1c-pg18-persistent-full |
| Oracle 23.5.0.24.07 | fail | 169 (partial capture) | m-s1c-oracle-full |
| Oracle 21.3.0.0.0, amd64 | fail | 169 (partial capture) | m-s1c-oracle-full |
| Oracle 11.2.0.2.0, amd64 thick worker | fail | 169 (partial capture) | m-s1c-oracle-full |

Oracle loads AREAS, then rejects required TYPE_EDGES with
`unsupported_column_type` (shared engine lacks CLOB/BLOB support). The full
fixture was seeded; no table was excluded to manufacture a pass.
Each Oracle version actually started its concurrent writer and product load.

T8 is **partial everywhere**: target logical bytes measured, model tokens zero;
source peak connections and aggregate duty remain unmeasured. No WAL/index/
TOAST/physical-write claim. Prior PG18 runs failed on Docker-daemon transport
errors and remain failed. A persistent acknowledged writer resolved the harness
churn; the successful retry uses fresh fixtures.

PG13-17 initially compared temporal/binary JSON as strings. Their original
reports are preserved. `typed-comparison/` re-audits restore typed values from
the same retained successful source/target SQL outputs and pass all 233 tables.
PG18's successful retry uses the typed oracle directly.

The full matrix tested the loader before the additive uint64 change. The final
build passes `m-s1c-final-smoke` (PG17, reduced 36 tables), and
`m-s1c-final-recovery` (real target, unsigned regression and both SIGKILLs).
This distinction is intentional; the final SHA was not rerun at full scale.

## Validation commands

```sh
cargo check --workspace --locked
cargo test -p oo-pipeline --locked
CARGO_TARGET_DIR=target cargo test --manifest-path tools/oo-load/Cargo.toml --locked
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools/oo-test -v
python3 tools/oo-load/test-target.py --run-id m-s1c-final-recovery
python3 tools/oo-test/t1.py --engine postgres --versions 13,14,15,16,17,18 --run-id m-s1c-pg-full
python3 tools/oo-test/t1.py --engine postgres --versions 18 --run-id m-s1c-pg18-persistent-full
python3 tools/oo-test/recompare_t1.py --evidence evidence/m-s1c-pg-full --versions 13,14,15,16
python3 tools/oo-test/recompare_t1.py --evidence evidence/m-s1c-pg-full --versions 17
```

Workspace check and pipeline tests pass. Loader default tests: four pass, two
fixture-dependent tests ignored; the dedicated harness actually runs both.
Python: 23 pass. Raw build/test output is in `m-s1c-20261104`. No pre-existing
test failure was suppressed or claimed as passing.

Result-file SHA-256:

```text
dd0a3be01db6e9519eb0eef734bb5e93131bec0a2d614362661af1e910bd1fec  m-s1c-pg-full/results.json
3000555e0e4c17629bcb8d5f7a9e1a36b1a966ccdfd2517f1f8aaccdabfedcbf  m-s1c-pg18-persistent-full/results.json
5280b55bc85d8ac9f734283204e754abb6f000e7b04d2ca9fadb8c4e683346fc  m-s1c-oracle-full/results.json
```

## Remaining exits and shared changes

**S1-M-01 remains not done.** The local target workaround is not the existing
legacy C5/jobs/release path, does not compose immutable parent plus changed
chunks, and does not publish Gateway releases. Real recovery verifies the new
local adapter, not legacy jobs acceptance. Admission remains experimental.

`shared-interface.patch` is an exact, unapplied proposal for optional
source-authenticated growth splitting. Current code works around its absence
by complete-table replan. This proposal only freezes a hook; engine overrides
and loader integration require coordinator review and tests. No shared patch
is needed to run the current workaround.

There is no honest small patch that turns MySQL-only legacy topology into safe
cross-engine admission or adds Oracle LOB streaming. Those require coordinator/
engine-owner work, including typed metadata, bounded LOB transport and real
fixture validation. No unsafe cast or source-engine relabelling is proposed.

S1-D-01 T1 enabling evidence is delivered; S1-E-01 is not enabled due to LOBs;
S1-C-01 has fake unsigned/watermark evidence only, not real MySQL acceptance.
No source resource-budget acceptance or online CDC consistency is claimed.
All containers and volumes created by these runs were removed; no lane-M
containers remain. Cached images were not removed.
