# M-S1e MySQL T1/T8 sign-off (2026-11-04)

Status: partial at the overall TODO boundary. The requested four-version
full-scale T1/T8 rerun and two real SIGKILL recovery exits pass. S1-M-01's
immutable-parent composition and shared/Gateway publication remain outside
this local admission path. No full product-loop acceptance is claimed.

Rebased on `v0.2/product-loops` at `e40db543f`, already containing `c9c2c5695`.
Only MySQL mapping and the generic/MySQL test harness changed. No Oracle hook,
shared file, dependency, lockfile, fixture gold or budget changed.

## TODO exits

- S1-C-01: done for this card. All four exact versions compare all 231 required
  tables with zero differences, including 2,000,000 events, 200 small tables,
  the 300-column table and type edges. A concurrent admin writer performs
  insert, update, primary-key update, delete and growth. It then quiesces for
  a corrective full reread. Completed-run resume adds no target writes.
- S1-C-02: done for this card. Each concurrent load, corrected load and resume
  passes T8: sampled reader peak 1 within topology budget 1 and aggregate
  active-reader duty within 25%. Target logical tuple bytes are recorded;
  no target-byte cap exists in ReadBudget. Model tokens are zero.
- S1-M-01: not done overall. Its two real process-kill recovery checks pass,
  but fake-source target idempotency and local view publication do not prove
  immutable-parent overlays, shared publication or Gateway acceptance.

## Accepted matrix

Commands use the unchanged full-scale defaults (no scale/table reduction):

```sh
python3 tools/oo-test/t1.py --engine mysql --versions 5.7.44,8.0.44,8.4.7,9.5.0 --run-id m-s1e-mysql-index-full
python3 tools/oo-test/t1.py --engine mysql --versions 8.0.44 --run-id m-s1e-mysql80-persistent
python3 tools/oo-test/t1.py --engine mysql --versions 9.5.0 --run-id m-s1e-mysql95-persistent
```

The first command exits 1 because its 8.0/9.5 writers fail; only its successful
5.7/8.4 entries are accepted. The two separate complete reruns exit 0.

| MySQL | Execution | T1 / T8 | Writer cycles | Max sampled duty | Target logical bytes | Accepted run |
| --- | --- | --- | --- | --- | --- | --- |
| 5.7.44 | amd64 emulated | pass / pass | 546 | 14.475% | 221,962,029 | `m-s1e-mysql-index-full` |
| 8.0.44 | arm64 native | pass / pass | 738 | 14.585% | 224,167,661 | `m-s1e-mysql80-persistent` |
| 8.4.7 | arm64 native | pass / pass | 942 | 19.531% | 222,114,698 | `m-s1e-mysql-index-full` |
| 9.5.0 | arm64 native | pass / pass | 1269 | 19.409% | 223,684,760 | `m-s1e-mysql95-persistent` |

Every accepted run uses product binary SHA-256
`0ed36eeec587b5481446b37c998a23fed42c59780c55878722eace94adc4f0c6`.
5.7/8.4 use the earlier per-cycle writer; 8.0/9.5 use the persistent writer
with the same mutations and explicit commit acknowledgement. No mutations
are retried. The read-only sampler retains bounded retries for confirmed
pre-dispatch Docker refusals, with all attempts preserved.

Consistency is B: quiescent final comparison, not online CDC convergence or
a cross-table transactional snapshot. Peaks and duty are sampled, not a
continuous upper-bound proof. Target bytes include committed logical stage
tuples, not WAL/index/TOAST/physical I/O. Resume checks durable receipt replay.

## Fixes and preserved failures

1. The original full run `m-s1e-mysql-full` passes required type mapping after
   the coordinator's SET/latin1 fix, but 5.7 fails `chunk_index_unused` before
   `unique_only`. 8.0 completes loading and resume but comparison encounters
   Docker's newer Unix socket refusal message. 8.4/9.5 pass on that older binary.
2. `m-s1e-mysql57-index-probe` is a reduced diagnostic, not scale acceptance.
   Real EXPLAIN shows that the converted expression's same-name alias makes
   ORDER BY use `ALL / Using filesort`. Removing the alias uses `code / index`
   with identical values. Readers decode by ordinal. The mapper now omits
   that alias while preserving the charset round-trip guard; unit coverage
   includes latin1, gbk and ucs2. Index protection remains unchanged.
3. `m-s1e-mysql80-retry` passes after recognizing the newer pre-dispatch
   read-only refusal. Its older product binary is superseded by the accepted
   matrix above.
4. The post-mapping-fix full matrix passes 5.7/8.4 but fails 8.0/9.5 when
   per-cycle writer Docker exec calls are refused. The harness now reuses
   its persistent admin-session writer for MySQL, disables reconnect and
   requires a commit marker per cycle. Missing acknowledgements still fail.
   Separate full reruns pass; original failures remain untouched.

## Recovery and checks

```sh
python3 tools/oo-load/test-target.py --run-id m-s1e-recovery-final
```

Two real SIGKILL exits are -9, after stage commit and after publication.
Final recovery exits 0. Each checkpoint has one receipt, three rows and
174 write bytes. This uses an owned PostgreSQL target and fake SourceEngine,
not a real-MySQL kill/recovery test. An earlier successful run is preserved
as `m-s1e-recovery`.

Builds use `CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target`
to limit disk use. Raw checks are in `evidence/m-s1e-build/`.

- `cargo check --workspace --locked`: passed before each commit.
- `cargo fmt -p oo-connectors`: passed; only the authorized map file changes.
- `cargo test -p oo-connectors --features mysql --locked -- --skip parent_deadline_kills_the_worker_and_the_next_call_completes`:
  297 passed, 24 ignored.
- The excluded `parse_file_cli::parent_deadline_kills_the_worker_and_the_next_call_completes`
  fails identically before the mapping change and on archived base `e40db543f`:
  `group_signal=rejected` versus `sent`. Evidence: `connectors-base-tests.log`
  and `base-deadline.log`. This pre-existing host failure is not weakened.
- `cargo test --manifest-path tools/oo-load/Cargo.toml --locked`: 6 passed,
  3 ignored by default. Target tests are separately exercised by recovery;
  the real split test was not rerun on this card.
- `python3 -m unittest discover -s tools/oo-test -v`: 29 passed, including
  both persistent writers' successful and missing-acknowledgement paths.
- `check_spec.py --self-test`, `check_plan.py --self-test`,
  `check_v02_plan.py --self-test`, staged secret/path scans and diff checks pass.

## Evidence integrity and handoff

SHA-256 of each accepted run's `SHA256SUMS`:

| Run / version | Manifest SHA-256 |
| --- | --- |
| `m-s1e-mysql-index-full/mysql-5.7.44` | `1234a1a04e25bb4a5976eca43e61c01d93e22576cc0f03f7360fb040d08ec3b6` |
| `m-s1e-mysql80-persistent/mysql-8.0.44` | `51d0dc7a9e5bc9ab80dc144a53a748abee8b6c9ab1e39d13b64e535484c12ef7` |
| `m-s1e-mysql-index-full/mysql-8.4.7` | `0bd2e5caf9c2dce31a8a5c4ef138d741dc10298d6b0a6c7ec07f3d485dfe613b` |
| `m-s1e-mysql95-persistent/mysql-9.5.0` | `8c5d11a68e6feac77c8296a5af44fef52d498780b8dac2118313f0cf00d5d1d6` |
| `m-s1e-recovery-final` | `f913e795499226208c2574668801989a46afd665428eacc828a6b7f520ea5b5a` |

The raw checksum audit is `evidence/m-s1e-audit/manifests.json`. Original failed
runs are also verified, not overwritten. All owned containers, volumes and
generated secret directories are removed; images remain cached.

Requested shared-file patches: none. Cargo.lock changes: none. The coordinator
owns overall S1-M-01 completion and acceptance/state updates. No customer
systems, unrelated services, Oracle hooks or other worktrees were changed.
