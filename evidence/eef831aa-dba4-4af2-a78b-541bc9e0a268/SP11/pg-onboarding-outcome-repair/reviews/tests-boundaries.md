# Review: tests and boundaries

Scope: `tools/pg-onboarding/tests/orchestration.rs`, `tests/failure_truth.rs`, and `src/json_doc.rs` unit tests. Compared runs are `linux/red-attempt-4` and `linux/green`.

`linux/red` failed to compile because the underscore `CARGO_BIN_EXE` name is unset. The final tests use the hyphenated name. `linux/red-attempt-2` confirmed the CLI then ran and printed `pg-onboarding: config config` for a duplicate password key. `linux/green` prints `pg-onboarding: config shape` and does not create the run directory. The secret bytes are absent from stderr.

The failed-child CLI starts from a sealed inspect, sets `mark-install` and `fail-provision`, and checks the marker, the static stderr, and `changes_may_have_been_applied`. That assertion fails on the archive and passes on the repair.

These passed on both the archive and the repair:

- `sha256_and_generation_vectors`
- `refuses_secret_symlink_and_existing_run`
- `fake_compilers_cover_both_phases`
- `preflight_deadline_refuses_before_run_dir`

`live_chain` compiled and remained ignored. No PostgreSQL container was created. The depth-31 and 49999-null documents stay schema detail `config` on the repair, which is the inside-cap behavior. The next object and the 50000-null document are `budget`.

Clippy on green was `cargo clippy --locked --offline --all-targets -- -D warnings`. `clippy.exit` is 0.
