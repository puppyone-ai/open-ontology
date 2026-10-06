# Report

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a295`. RT-D04 / SP11-W02. Source commit `524c6f5197a0e31e8d9e8f3e06510c7ee3d1d823`. Crate tree `2a408d25eca5a076b1f6295b7ce40b8797f70526`. Parent before that commit `226eedeed6b7625669580116e3cab6ebd270738f`. Fixed archive `c8588faab992af1193ff58dbc489a0b117796aa0`.

`tools/session-release` is a standalone Rust workspace. It drives the accepted `session-store` adapter and the `session-transition` reducer. It persists `cancel`, then runs a direct-child harness stop and a dedicated-role PostgreSQL backend cancel concurrently. After the child has exited with no escaped descendant, it collects bounded outputs and transcript and destroys one UUID-owned directory. Evidence hashes are the in-crate SHA-256 of the JSON preimage, stored with `record_release_step`.

Exit 0 is `released_unclosed` / `local_fixture_not_certified`. The phase remains `releasing`. `physical_release_certified`, `auth_provider_used`, `runtime_used`, and `production_sandbox` stay false. This report does not accept the parent task and does not certify a physical release.

## Recipe lineage

Precode launch parent: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a275/SP11/session-store/linux/precode/launch.sh`, recorded sha256 `46fc53cd48dd65c97860b7bda63295f0e59909b54e9a1836925c1dd9e2e35341`.

a204 launch `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841`. a199 launch `bf2a422d751db818cdd4d852d14be00436d4177cb584b85131a4774453413f55`. Postgres `up.sh` `96613a159f6c05b4a8d8d1fb79bb2613ae9b3e9a00a7b942196548a00964da45`. `pg-wrap.sh` `25712207f986b11e122bd992d5a695881e1e86625e0edf2d0b683b023ebfdd2f`.

Impl changes from that precode recipe: new target volume `oo-a295-eef831aa-target-impl`, overlay of `tools/session-release` onto a writable copy of the fixed archive, readonly fixture mount, `OO_ROOT_LOCK` pointing at the archived root lock, signal probe, then `cargo test` and `cargo clippy` `--locked --offline`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Postgres image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. One builder at a time, 6 GiB, 1 CPU. One private database. No pull.

## Precode and database

`linux/precode/docker-run.exit` 0. `test-transition.exit` 0 (12 passed). `test-store.exit` 0 (11 passed, `--lib`). `absence.exit` 0. Rustc 1.94.0, `aarch64-unknown-linux-gnu`.

`pg/up.exit` 0. Container `oo-a295-pg`. `ports.json` is `{}`. Caps drop `ALL` and add `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETUID`, `SETGID`. The installer, not the orchestrator, creates `oo_session_admin`, `oo_session_runtime`, `oo_fixture_query`, and `oo_session_cancel`, and grants the query role to the cancel role.

## Decisive run

`linux/impl-5` container `c6b26f5abe2bc163fbec0ad7659f5a0a0e06ba3d56421ddf55b7fc5d32b368cd`.

| File | Exit |
|---|---|
| `docker-run.exit` | 0 |
| `signal-probe.exit` | 0 |
| `archive.exit` | 0 |
| `version.exit` | 0 |
| `clippy-component.exit` | 0 |
| `test.exit` | 0 |
| `clippy.exit` | 0 |

`hostconfig.txt`: ExitCode 0, OOMKilled false, Privileged false, NetworkMode `oo-a295-eef831aa-net`, CapDrop `["ALL"]`, Memory 6442450944, NanoCpus 1000000000.

Lib: 9 passed. Binary unit harness: 0 tests. Live: 9 passed in 15.00 s. Lock subset: 1 passed.

Live names: `blocked_query_and_slow_child_release_without_closing`, `cancel_cannot_treat_an_idle_transaction_as_stopped`, `cancel_role_is_limited_and_runtime_cannot_create`, `escaped_grandchild_and_canary_are_not_confirmed`, `inaccessible_postgres_does_not_spawn_or_close`, `missing_workspace_and_foreign_path_have_no_side_effects`, `noisy_transcript_is_not_confirmed_and_workspace_remains`, `restart_after_the_orchestrator_is_killed_does_not_signal_or_certify`, `revision_conflict_and_event_id_conflict_do_not_close`.

`linux/impl-5/source-sha256.txt` matches the sixteen committed crate files (0 mismatches).

## Preserved failures

`linux/impl` exit 101. Compile `E0614` because `into_iter` already copied `StepId`.

`linux/impl-2` exit 101. The unit fixture left `event_prefix` as `&str`. Clippy rejected unread harness path fields and a nested `if`.

`linux/impl-3` exit 101. Eight live tests passed. The idle-transaction test saw harness elapsed 5008 ms and backend elapsed 5007 ms because the harness timestamp was taken after the join. Clippy rejected the unreaped canary.

`linux/impl-4` exit 101. Clippy exited 0. Bootstrap returned `BootstrapConflict` for session ids already inserted by impl-3.

Impl-5 gives each live session a fresh id and records harness elapsed on the blocking thread when `stop` returns.

## Bounds that the decisive receipts show

The success receipt confirmed S1–S4 and S8, left S5–S7 and S9–S11 absent, destroyed the workspace, and refused a later `BeginTurn` with `AdmissionStopped`. Backend elapsed was at most 5000 ms. Harness elapsed was at least 5000 ms and larger than the backend elapsed.

The idle-transaction receipt was exit 2, `slo_miss`, backend basis `still_active`, S2 absent in the reducer, backend elapsed at least 4900 ms, and harness elapsed under 2000 ms. The cancel role then read state `idle in transaction`.

The restart receipt was exit 2, `resumed`, `deadline_basis` `wall_epoch`, S1 `harness_not_owned`, workspace still present, and the recorded pid still alive.

Port 1 was exit 3, `pg_unavailable`, phase still `Running`.

Full raw stdout and stderr are under `linux/impl-5/`. Earlier consoles are under `linux/impl`, `linux/impl-2`, `linux/impl-3`, and `linux/impl-4`.
