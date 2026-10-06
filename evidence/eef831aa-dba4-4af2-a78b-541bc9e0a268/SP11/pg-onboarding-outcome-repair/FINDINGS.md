# Findings

## Preserved failures

`linux/red` compiled the fixed archive plus the new tests and stopped before assertions. `test.exit` 101. `src` did not contain `changes_may_have_been_applied`. `env!("CARGO_BIN_EXE_pg_onboarding")` is unset. Container `c02f4450a4df86ff944b92320a540f2167584e02d8b9028e67c9cebdd3ab6fc7`. The log is `linux/red/test.log`.

`linux/red-attempt-2` reached assertions. Duplicate keys and the CLI stderr were `config`, where the tests require `shape`. Cargo's default fail-fast did not run `tests/orchestration.rs`. Container `3a6a26f1459dbbd2a40444e9f6d38e936bbb511cb2d9ccfff61ed9c108ca14e5`.

`linux/red-attempt-3` ran every test binary. Init-pg timeout, catalog failure after import, and the failed provision CLI still wrote `unchanged_by_this_tool`. Duplicate manifest and duplicate decisions were accepted (`unwrap_err` on `Ok`). The preflight test had already seen step `config` and detail `config`, then panicked because `outcome.json` was absent. Container `b1cd785ab502bc396e772349da6ba16e427e29b46307e42f234c50a08209c2d8`.

## What the green run includes

The CLI tests resolve `option_env!("CARGO_BIN_EXE_pg-onboarding")` and then the underscore form. This Cargo provides the hyphenated name.

`linux/red-attempt-4` is the red run of the final tests. `preflight_deadline_refuses_before_run_dir` passed on the unrepaired archive: deadline `0` is `config`/`config` and the fresh run directory is absent. The other new outcome and JSON tests failed. `fake_compilers_cover_both_phases`, `refuses_secret_symlink_and_existing_run`, and `sha256_and_generation_vectors` passed. Container `af276b748d76aa9369781494799f8b33cc01c06b1d49bb381aeaed22ca882d4d`.

`linux/green` is the repaired tree at the bytes committed as `50d4e705a2bb4ba014919ef1e071da065cf8e9b7`. `test.exit` 0. `clippy.exit` 0. Container `536af1d9655fb2710d05281e66642612a0ebcad2583af077c9728228e1d982a6`.

## Behavior that the tests lock

`spawn` sets the phase flag only after `Command::spawn` returns `Ok`. Secret refusal and a deadline rejected by `millis` happen before that. Config validation, including a deadline outside 1..=180000, runs before `create_dir`, so no outcome file exists.

A failure outcome written with the flag clear uses `unchanged_by_this_tool`. Duplicate decisions are in this set: the apply outcome is written, and `review-pg-draft.stdin` is not. A duplicate manifest returns before `apply_in`, so no apply outcome is written.

A failure outcome written after any successful spawn uses `changes_may_have_been_applied`. The init-pg deadline, a catalog failure after import, and a provision failure after install are in this set. `database_rollback` and `database_release` are false. The install side-effect file `side-effect-marker-ok` remains. The process stderr is `pg-onboarding: provision-mapping-reader-clerk exit` and does not contain the marker or the fixture password.

The visitor rejects a repeated key after escape decoding, a document longer than the cap, depth above 32 with the root counted as 1, more than 50000 values, and a non-whitespace trailing byte. Operator documents use 1048576 bytes. Child captures stay at 8388608 bytes. `serde_json` is not the duplicate-key authority. It still builds the value after the visitor accepts.

## Limits

There is no separate fixture whose compiler stdout contains a duplicate key. That stdout is parsed by the same visitor after spawn, so a duplicate there fails the step with `changes_may_have_been_applied`.

The ignored live chain was not run. a261 `linux/chain-attempt-2` remains the earlier live record of the unrepaired tool. This repair does not extend that record.

No outcome in this repair drops a role, drops a schema, rolls a database back, or publishes a release.
