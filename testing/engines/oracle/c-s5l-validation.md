# C-S5L: bounded Oracle LOB range digests

Rebased on `51cec087d`. Scope: the Oracle LOB C5 defect on S5-E-04 and
S5-H-01, not completion of those plan rows' entire fault/schema/P0 matrix.

## Product behavior

LOB-containing ranges reuse the snapshot page reader, including its bounded
locator reads, canonical-byte limit, deadline and thin/thick transports.
Each range uses one SCN; physical/keyless ranges retain the plan SCN. Typed
row SHA-256 lanes are summed without retaining range payload. NULL, empty
locators, field boundaries, Unicode, binary content and duplicates remain
distinct. No DBMS_CRYPTO grant is required. Scalar SQL digests are unchanged.
The digest API uses the existing default ReadBudget: 100,000 rows, 32 MiB
canonical bytes and 30 seconds. Over-budget ranges fail, never truncate.

## Full-scale corrected T1

Command:

```sh
OO_TEST_LANE=c PYTHONDONTWRITEBYTECODE=1 python3 tools/oo-test/t1.py \
  --engine oracle --versions 23,21,11 \
  --worker-image oo-e-oracle-worker:e-fix-worker-v4 \
  --run-id c-s5l-t1-full-20261105
```

The unmodified corrected T1 defaults are 2,000,000 EVENTS rows and 200 small
tables. Each version compares all 231 tables, after a concurrent baseline,
no-rewrite replay, and exactly one product C5 correction. Each also passes
keyless digest-only and periodic full-read idle cycles with zero target
writes, payload, replacements and new generations. T8 passes all invocations.

| Actual version | Transport | T1 | T8 | Equal tables | EVENTS rows per side |
| --- | --- | --- | --- | --- | --- |
| 23.5.0.24.07 | thin, arm64 | pass | pass | 231 | 2,000,000 |
| 21.3.0.0.0 | thin, amd64 emulation | pass | pass | 231 | 2,000,000 |
| 11.2.0.2.0 | thick worker, amd64 emulation | pass | pass | 231 | 2,000,000 |

All runs use binary SHA256
`ebc08557070936e41e8e1fab2bdcf617626969349c9930e0b6c626b3c95c7648`.
SHA256 of each `oracle-VERSION/t1-t8.json`, in table order:

```text
c5121e24bb812d5f5aa6908be98cb92c90a63b1c08de0b13bcd41da8f1a6563e
3e56040234bec885ded6511f1bb0e213000c34986aa259380ebbd4cea2ab1968
a5f209929e092f43dceba3fd080cd13b703790d7fd207fcb7659eeaaf1b097b8
```

## Focused LOB checks

`evidence/c-s5l-20261105/run-lob.py` invokes
`testing/engines/oracle/run.py --lob-only --lane c` for 23/21/11, pinning 23
explicitly to the same 23.5 image as T1, and using the thick worker for 11.
All three pass `oracle_lob_budget_and_hash` in `c-s5l-lob-20261105`:
multichunk CLOB/NCLOB/BLOB content, Unicode, empty versus NULL, IPC hash,
4096-byte refusal, suffix-only mutation detection for all three LOB types,
and idle C5 cycles with zero replacements/payload. The test sink now handles
typed Arrow Binary values instead of assuming every noninteger is Utf8.

## Local checks and preserved failures

Raw logs: `evidence/c-s5l-20261105/`.

- `cargo fmt -p oo-connectors`; `cargo check --workspace --locked`: pass.
- `cargo test --locked -p oo-connectors --features oracle --lib`: 171 pass,
  one live test ignored; new tests cover SQL bounds/SCN/overflow probes and
  digest order, batching, multiplicity, NULL/empty and full LOB content.
- Full connector suite with only
  `parent_deadline_kills_the_worker_and_the_next_call_completes` filtered:
  281 pass, 28 ignored. The unfiltered `parse_file_cli` test fails identically
  on archived base `51cec087d` (`base-failure.log`): macOS process-group
  signal is `rejected`, expected `sent`. No assertion was weakened.
- Oracle Python test files in separate processes: 17 pass. Combined discovery
  collides between two modules named `audit`; `test_t1_hook` and
  `test_typed_acceptance` import failures also reproduce on the archived base.
- Proposed shared collector patch: existing audit unit tests and explicit
  32,767-character wrapping/truncated-JSON regressions pass locally.

## Shared audit integration

The unmodified shared T7 runner fails all three versions in
`c-s5l-t7-20261105`, although their T1/T8 workloads pass: SQL*Plus wraps wide
SQL audit JSON at 32,767 characters, and the line parser rejects the split
record. These failed reports are preserved. Decoding the retained 23/21
outputs without physical line wraps recovers every row with zero truncated
SQL values. Separately, T7's coverage check still expects three snapshot
loads; corrected T1 now has baseline/replay plus C5 correction/idle windows.

`c-s5l-shared-audit.patch` is the exact requested coordinator-owned diff;
no shared source was edited. It parses complete JSON objects across physical
wraps (still rejects incomplete JSON) and requires successful typed T1,
two baseline/replay windows and three C5 correction/idle windows. It does
not relax the SQL classifier, privilege checks, gold values or write rules.
The local wrapper `evidence/c-s5l-20261105/run-audit-corrected.py` applies
these two changes in memory and reruns the reduced audit fixture (1,000
EVENTS rows, three small tables); it is not another full-scale run.

All three corrected-wrapper audits pass snapshot and C5 with read-only
privileges and zero rejected records:

| Version | All statements | C5 statements | Rejected |
| --- | --- | --- | --- |
| 23 | 33388 | 12126 | 0 |
| 21 | 33282 | 11913 | 0 |
| 11 | 32043 | 13648 | 0 |

SHA256 of each corrected `t7-snapshot.json`, in version order:

```text
ae74f9d52642728d1129131f4b451184f45fadf47b0ee900c6e82d18d99f60df
f23e9e67adcccec59e4024a01262cfbf3bcd17cfb71c3de27dcc214b43b1c06e
e6b3bf5aa0100ca96b6e3b310e79f86e4d173aca8a2aecf5ca955e55f1ef7413
```

The shared runner remains unfixed pending coordinator integration; these
are explicitly local-wrapper results, not passes of the unmodified runner.
Both TODO rows remain open at plan level; the LOB defect, full-scale T1 and
idle-cycle exits on this card are met. No Cargo.lock, migrations, shared
engine interfaces, docs/specs, support matrix or execution state changed.
All created containers and volumes were removed; none left running.
