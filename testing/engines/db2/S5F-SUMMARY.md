# E-S5f Db2 implementation and acceptance

Plan: S5-E-03. Run: `e-s5f-20261105`. Base: `45039bf52`.
`git rebase v0.2/product-loops` was up to date.

## Implementation

- Opt-in `oo-connectors/db2`, no new Rust dependency, no Cargo.lock change,
  no IBM native library in the default artifact.
- Customer-provided IBM CLI through an isolated Python `ibm_db` 3.2.6 worker,
  versioned JSON controls and framed Arrow IPC. Local test image:
  `sha256:2b86cf46be8f802cbe3ed6e4dbe4f92964d56bdcc98d2871b979bd8c029a3716`.
  This is not permission to redistribute the native IBM client.
- Version/capability probe, scoped SELECT checks with executable DBA advice,
  source-ordered scalar/composite primary-key chunks, bounded no-key read,
  deadline/row/byte limits, duty-cycle pacing, registered plan validation,
  schema-bearing empty reads, C5 range digests and observation watermarks.
- Integer values use Arrow Int64; decimal and temporal values retain exact
  text. Binary/LOB/unknown/distinct types and unsupported key types fail
  closed. No native decimal/temporal target typing, log CDC, global snapshot
  or Db2 12.x acceptance is claimed.
- `oo-load --db2-worker` uses the existing durable Agent DB target.
  SSH and scheduled CLI C5 are rejected rather than silently downgraded.

## Runtime evidence

Db2 **11.5.9.0** ran on linux/amd64 under Docker Desktop emulation.
The worker connected as the dedicated `OOREADER`; fixture administration
and concurrent mutation used a separate instance-owner connection.

Connector tests:

```sh
OO_DB2_EVIDENCE="$PWD/evidence/e-s5f-20261105" \
  cargo test -p oo-connectors --features db2 --lib --locked \
  real_db2_keysets_digest_budget_and_empty_schema -- --ignored --nocapture
OO_DB2_EVIDENCE="$PWD/evidence/e-s5f-20261105" \
  cargo test -p oo-connectors --features db2 --lib --locked \
  real_db2_missing_grant_and_c5_change -- --ignored --nocapture
```

Both passed when run against their documented fixture phase (small seed
and then full seed respectively). Evidence: `engine-acceptance.json` and
`t0-c5-acceptance.json`. This verifies connector probing, missing SELECT
advice, integer/composite/no-key/empty reads, budget refusal before sink
publication, stale chunks, idle digest stability and changed/restored range
digests. It is **not** a product `oo doctor` T0 pass.
The final live regression also verifies that row-budget overflow drains
the bounded completion frame and permits a subsequent probe on the same
worker session (`t0-c5-overflow-regression.log`, exit 0). That last overflow
fix was validated by the focused live regression and crate tests; full-v4
used the preceding successful-path binary, identified in
`artifact-identities.json`, and was not rerun after the overflow-only fix.

Full source-to-Agent-DB command:

```sh
python3 testing/engines/db2/target_acceptance.py \
  --run-id e-s5f-20261105 --attempt full-v4 --full
```

`full-v4/target-acceptance.json`: **T1 and T8 pass**; all 231 tables compare
equal, including 2,000,000 event rows. The admin writer completed 33 mutation
cycles, with 33 reader-connection samples and observed reader peak 1.
The corrected cut wrote 198,197,828 logical target bytes; resume did not
rewrite. No model was called. See `full-v4/index-rows.json` for explicit
T0=skip, T1=pass, T8=pass rows. The corpus has a 300-column
wide table. The admin writer performs insert, update, delete and PK moves.
B-consistency correction explicitly stops that writer and rereads the
whole cut before exact source/Agent DB comparison. The run also checks
no-rewrite resume, source reader connection samples, paced reads, committed
target bytes and zero model calls.

## Preserved failures

- Initial bind-mounted instance-home creation failed; the owned fixture
  was initialized with container-local homes instead.
- First worker image download hit TLS bad-record-MAC; a normal retry
  succeeded without weakening TLS.
- Initial worker handshake failed because IBM CLI DSN credentials must be
  in the connection string; the corrected worker receives them via stdin,
  rejects DSN delimiters and never echoes driver diagnostics.
- Single-transaction 2M seeding exhausted the fixture log. The fixture
  generator now uses 100k-row admin transactions; no server log limit was
  weakened.
- Early comparison attempts exposed CLP prompt/no-row handling defects in
  the harness. Full-v2 comparison psql exited 137 after the full imports;
  full-v3 used cursor fetching but stopped on a silent empty-table EOF.
  Full-v4 uses `FETCH_COUNT=1000` and the explicit CLP no-row status.
  All failed attempts remain separate; their verdicts are not passes.

## Local checks and integration boundary

- `cargo check --workspace --locked`: pass.
- Connector crate: 235 active tests pass, 25 ignored live tests, excluding
  one proven baseline failure. Unit subset includes 125 tests.
- `cargo test --manifest-path tools/oo-load/Cargo.toml --locked`: 38 active
  tests pass; 7 existing live tests ignored.
- Db2 Python tests: 14 pass. DM8 builder tests: 6 pass.
- `cargo fmt` checks for touched crates and all three spec/plan checkers pass.
- Cargo deny passes advisories/bans/licenses/sources; audit reports zero
  vulnerabilities. These scans cover the Rust graph, **not IBM native CLI**.
- Full connector run fails
  `parse_file_cli::parent_deadline_kills_the_worker_and_the_next_call_completes`
  on macOS (`group_signal=rejected`, expected `sent`). The identical failure
  was reproduced on an archived base `45039bf52` inside this worktree;
  see `base-parse-file.log`. It was not fixed or weakened.

The card remains partial until coordinator-owned `oo doctor` dispatch is
integrated and actual product T0 runs. The current shared target mapper
also misidentifies Db2 11.5 as Oracle 11g; the lane verdict writer uses
`db2-luw-11_5` explicitly and never promotes connector probe into T0.
The exact proposed diff is `coordinator-integration.patch`, validated with
`git apply --check`, **not applied or runtime-tested**. No shared source,
lockfile, support matrix, service, token file or customer system was changed.

Cleanup: both source and Agent DB containers were removed. The final
`oo.run_id=e-s5f-20261105` inventory is empty (`cleanup.json`). Local pinned
images and ignored evidence remain; no image was pushed.
