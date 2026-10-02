# Handoff: target fence and apply receipts

SP04-W05-a is the schema and database enforcement in this commit. The parent task, AT-PL-05, and the import vertical are still open. Review this commit by its own SHA. The post-smoke snapshot parent was `78d106fcb3e7e1705b617ed6df58933d85297e7b`.

## Owned paths

- `crates/oo-pipeline/migrations/`
- `crates/oo-pipeline/tests/`
- `contracts/pipeline/target-fence-receipts/`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a072/SP04/target-fence-receipts/`

`crates/oo-pipeline/src/lib.rs`, `crates/oo-pipeline/Cargo.toml`, the root manifest, and `Cargo.lock` were not changed. `a068` jobs files were not changed.

## Checks that passed

From-empty PostgreSQL 17.10 run: `fixture-fresh/harness.log` ends with `fixture_result=pass` against expand SHA-256 `97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9`. The earlier in-place resume is `fixture/harness.log`. The `42703` failure before the trigger fix is `fixture/attempts/fence-flow-new-field/`.

Linux arm64 MSRV 1.94 `oo-pipeline` test and clippy: `pre-smoke/phases/` and `post-smoke/phases/` are all exit 0. The post-smoke test run executed `target_fence_files_match_the_published_contract`.

## Reproduce

On an owned PostgreSQL 17.10 container with the pinned image and no host port:

`crates/oo-pipeline/tests/provision_pg.sh`

`crates/oo-pipeline/tests/pg_fixture.sh <evidence-dir> <container-name>`

Leave `OO_FIXTURE_FROM` unset. The harness resets its own databases, applies the two SQL files, and checks SQLSTATE on separate login sessions.

Rust, from the focused-check template, with `RUSTUP_TOOLCHAIN=1.94.0`:

`cargo fetch --manifest-path Cargo.toml --locked`

`cargo test --manifest-path Cargo.toml --locked --offline -p oo-pipeline`

`cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-pipeline --all-targets -- -D warnings`

## Next implementation

The Loader still has to open one dedicated Agent connection, call `adopt_target_generation`, and commit user data, quality rows, and `commit_apply_receipt` in the same target transaction. Losing that connection must stop writes. The next owner writes only after it acquires the target lock and receives a new generation.

Still absent: the Loader data-write API, CDC, the control-database receipt mirror, reject-event durability, and stream acknowledgement. Do not enable imports against source `registration_allowed` until that source defect is fixed. Do not treat a jobs control lease as this fence.

## Cleanup

Owned containers, networks, and volumes listed in `cleanup.txt` were removed after the logs and phase exits were on the host. `oo-a079-pg1710` was left running. No prune was run.
