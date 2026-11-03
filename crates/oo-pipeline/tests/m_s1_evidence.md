# M-S1 author evidence

Run: `m-s1-20261104`. Raw logs are git-ignored under `evidence/`.

`cargo check --workspace --locked` passed.
`cargo test -p oo-pipeline --locked`: 341 passed, 53 explicitly ignored.
`check_spec.py`, `check_plan.py`, and `check_v02_plan.py` passed document checks;
they do not certify product acceptance.

## S1-M-01: partial

- `python3 crates/oo-pipeline/tests/c5_recovery_pg17.py --evidence evidence/m-s1-20261104/pg17-verified`
  passed on PostgreSQL 17.11. The fixture creates a network-isolated
  `postgres:17` container and labelled volume, then removes both.
- Two actual child-process SIGKILLs occur after private staging and after
  target commit. The real jobs API reclaims the same task at epochs 1, 2, 3.
  The final target has 100,002 exactly compared rows, one range apply, two
  private-part receipts, and unchanged parent cells.
- Five durable meter records total 27,433,619 committed logical tuple bytes.
  Both replay and rollback add zero records. This is not a WAL, index, TOAST,
  metadata, or disk-allocation measurement.
- The unpublished view is empty to a non-owner; the owner performs the exact
  comparison. No source-admission, publication, ACK, or Gateway claim is made.
- Summary SHA-256:
  `91b1326564ecf6fb61e4c5a3f88c6b27794f7a77fd94b60fae413ff96b5c98d8`.
  Its manifest hashes the harness, installed SQL, and fixture helpers.
- Earlier failed harness attempts remain in the same evidence directory.
  They exposed a missing backend fence in the fixture, repeated evaluation of
  a composite claim function, and reading through the closed publication gate.
  The fixes use the production stage/apply/rebind ordering and owner-only
  inspection; expected data and comparison assertions were not weakened.

`engine_chunk::decode_chunk` validates engine receipts and sealed IPC into the
existing loader's `DecodedStream`. Tests use fixed synthetic producer receipts,
including empty, stale, swapped, corrupt, malformed, and over-budget chunks.
This is a content adapter, not a complete `SourceEngine` runner or admission.

The remaining engine-neutral loader exit is **not met**:
`native_c5_parts::PartScope::validate`, migration 0103's `c5_parts_open`,
migration 0104's `c5_parts_stage_bound`, and
`native_update/range_parts.rs::authenticate` require MySQL-bound scopes.
`ChunkReceipt` has no authenticated source binding, schema fingerprint, or
snapshot identity. Engine `[lo, hi)` bounds also cannot simply be relabelled
as legacy source-authenticated C5 topology. Complete this with the coordinator's
source-admission contract and an actual fake-`SourceEngine`-to-target test;
do not weaken the existing admission checks.

## S1-M-02: done at author-test level

`cargo test --manifest-path tools/source-onboarding/Cargo.toml --locked -- --test-threads=1`:
210 passed, 2 explicitly ignored. Existing tests prove slow-slot refill,
failure draining and receipt retention, immutable deadlines across resume,
interrupted budget commits, and real elapsed downtime. Added tests exercise
all widths 1-4, reject 0/5, and distinguish source/database groups.

Default-parallel runs are flaky:

- `refresh_incremental::tests::concurrency_groups_shared_source_watermarks_and_resume_does_not_rescan_successes`
  fails with `WouldBlock` on both this change and base `ac947b531`
  (`source-base-recheck-3.log`).
- `full_pipeline::tests::runtime_preflight_refusal_is_first_even_with_a_cached_provision_receipt`
  once failed with `immutable_output_drift`; not reproduced on base.

The complete serial suite passes without changing either assertion. Existing
formatting outside the changed regions was preserved after running cargo fmt.

No shared-file patch or Cargo.lock change is included. Migration 0119 is new;
all prior migration bytes are unchanged. Coordinator acceptance remains open.
