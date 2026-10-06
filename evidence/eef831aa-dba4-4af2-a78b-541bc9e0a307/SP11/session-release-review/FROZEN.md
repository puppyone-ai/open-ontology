# Frozen review scope

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a307`. Reviewer session is independent of author `eef831aa-dba4-4af2-a78b-541bc9e0a295`. Fixed commit `72dc9e2db841de4de368216bbbe13d738f850ea9`. No product edits. No new product test, and no second child program, until this file exists and a later concrete failure forces a new frozen child.

## What is executed

One private Postgres 17.10 and one Linux arm64 Rust 1.94 builder. The builder archives that commit (`Cargo.lock`, `tools/session-release`, `tools/session-store`, `tools/session-transition`) and runs the crate already in the archive. It does not overlay the working tree.

Pre-overlay command, before any review file is copied into the crate:

```text
cargo test --locked --offline --all-targets -- --test-threads=1
cargo clippy --locked --offline --all-targets -- -D warnings
```

That is the author's 9 library tests, 9 live tests, and 1 lock-subset test. The live tests are the owned-child fixture. They already name the behaviors this review has to see:

- `blocked_query_and_slow_child_release_without_closing`: persisted cancel, concurrent backend cancel and direct-child wait, 6s child versus the 5s backend branch, bounded collection, UUID workspace absence, durable step hash, phase `releasing`, no `complete_release`, S5–S7 and S9–S11 absent.
- `cancel_cannot_treat_an_idle_transaction_as_stopped`: `idle in transaction` does not confirm S2. The unit test `cancel_return_and_socket_drop_are_not_success` rejects `socket_closed`. No extra socket-cut campaign.
- `restart_after_the_orchestrator_is_killed_does_not_signal_or_certify`: SIGKILL, restart, `harness_not_owned`, phase stays `releasing`, recorded pid stays alive.
- `revision_conflict_and_event_id_conflict_do_not_close`.
- `noisy_transcript_is_not_confirmed_and_workspace_remains`.
- `missing_workspace_and_foreign_path_have_no_side_effects`.
- `cancel_role_is_limited_and_runtime_cannot_create`.
- `inaccessible_postgres_does_not_spawn_or_close`.

`escaped_grandchild_and_canary_are_not_confirmed` stays in the suite because it is one of the 19. It is not a new attack set.

## What is not a new accepted unit

`tools/lock_triples.rs` is a review oracle, compiled with `rustc` inside the builder, not added to the crate. It checks every registry triple `(name, version, checksum)`:

- session-release equals session-store
- each of those triples is in the archived root lock
- each session-transition registry triple is in the root lock and in the session-store lock
- path packages `session-release`, `session-store`, and `session-transition` are not registry packages
- the three locks in the work tree still match the archive

No `cargo update` and no `cargo generate-lockfile`.

## proc_snap residual

No live fault injection. `harness::spawn` is crate-private, and `/proc` is readable in this container. The source check is: after `Command::spawn`, a failed `proc_snap` returns `proc` while `Child` drops without a signal; `fresh` records that reason and does not confirm S1 or set `actual_absence`. That is a residual, not success, and not absence. A new child that tries to hide `/proc` is out of scope.

## Credentials

Passwords are generated under `/tmp/oo-a307-eef831aa-dba4-4af2-a78b-541bc9e0a307` and mounted as files. They are not docker environment variables, not process arguments, and not written into this evidence directory. Scripts must not print argv or environ.
