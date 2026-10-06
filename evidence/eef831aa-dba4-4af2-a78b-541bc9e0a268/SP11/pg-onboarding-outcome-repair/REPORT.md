# pg-onboarding outcome repair

Session `eef831aa-dba4-4af2-a78b-541bc9e0a268`. Fixed product archive `56710cc73cfe1663272ecaa503984e1f663428ad`. The owned tool commit is `50d4e705a2bb4ba014919ef1e071da065cf8e9b7`, parent `efdffed87f4e19070536cbd6a72fa1a6583e639e`. This report covers `tools/pg-onboarding` and this evidence directory. It does not accept a parent unit, semantic publication, authentication, natural language, or a release.

## Result

The repaired tool writes `database_state=changes_may_have_been_applied` on a failure outcome after any child `Command::spawn` has returned. `database_rollback` and `database_release` stay false. Step files and an external side-effect marker stay in place. A deadline outside 1..=180000 is rejected in config validation before the run directory exists, so no outcome file is written.

Operator config, the inspect manifest, the inspect outcome, reviews, decisions, visibility files, and child JSON go through a duplicate-rejecting visitor before `serde_json` builds the value. Duplicate keys, including escaped duplicates, are detail `shape`. Byte, depth, and node overflows are detail `budget`. Trailing non-whitespace is `shape`. Trailing whitespace is accepted.

The success statuses are unchanged: inspect `awaiting_review`, apply `ready_for_trusted_query`, with `database_state=child_compilers_applied_no_release`. Those words are the existing outcome strings. They are not semantic publication, authentication, natural language, or a harness mount.

No PostgreSQL was started. The a261 live chain evidence is unchanged and was not rerun. `tests/live_chain.rs` stayed ignored.

## Proof

Linux arm64 Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Toolchain text `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Each new builder used 6 GiB, 1 CPU, capability drop `ALL`, and the private network `oo-a268-eef831aa-net`. PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb` stayed unused.

| Run | Exit | Record |
| --- | --- | --- |
| smoke | 0 | Fixed archive, 3 existing tests passed, `live_chain` ignored. Container `3edb30571931c42dc215414e54fbf805d3096f4999c1fa8f344952342b038fa0`. |
| red | 101 | Compile stopped: `CARGO_BIN_EXE_pg_onboarding` is unset on this image. Container `c02f4450a4df86ff944b92320a540f2167584e02d8b9028e67c9cebdd3ab6fc7`. |
| red-attempt-2 | 101 | `failure_truth` assertions failed (`config` versus `shape`). Cargo stopped before orchestration. Container `3a6a26f1459dbbd2a40444e9f6d38e936bbb511cb2d9ccfff61ed9c108ca14e5`. |
| red-attempt-3 | 101 | Full suite. Outcome and duplicate tests failed. The preflight test expected an outcome file the archive does not write. Container `b1cd785ab502bc396e772349da6ba16e427e29b46307e42f234c50a08209c2d8`. |
| red-attempt-4 | 101 | Same tests as green, on the unrepaired archive. 11 `failure_truth` failures, 5 orchestration failures, 4 orchestration passes including the 3 existing tests and the preflight refusal. Container `af276b748d76aa9369781494799f8b33cc01c06b1d49bb381aeaed22ca882d4d`. |
| green | 0 | Repaired tree. 4 visitor tests, 11 `failure_truth` tests, 9 orchestration tests, `live_chain` ignored. Clippy `--locked --offline --all-targets -- -D warnings` finished with no warning. Container `536af1d9655fb2710d05281e66642612a0ebcad2583af077c9728228e1d982a6`. |

`Cargo.toml` and `Cargo.lock` match the fixed commit. Pins stay `serde` 1.0.229 with `derive` and `serde_json` 1.0.151. No `cargo update`. No product crate edit. No root manifest or root lock edit.

## Scope

Owned paths are `tools/pg-onboarding/**` and this evidence directory. The contract in the tool tree is `tools/pg-onboarding/CONTRACT.md`. The historical a261 contract was not edited.
