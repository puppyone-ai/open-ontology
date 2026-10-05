# Trusted SQL trial runner

Session `eef831aa-dba4-4af2-a78b-541bc9e0a231`. Fixed archive `934c8098703b6462c4f774878d1a4241bd84905d`, tree `ab3b296d4cf51b59cdfc543f31bc4d62fe4385b3`. Repository HEAD recorded at the proof launch is `56c506b8b061783873634486cbc39a3888b0e6bc`.

The owned product path is `benchmarks/sql-runner/**`. It is a standalone workspace. The root `Cargo.toml`, the root `Cargo.lock`, and every other product crate stayed on the archive. `cargo update` was not invoked (`linux/proof/cargo-update-invoked.txt` is `no`). Lock drift against the archived root registry pins is empty in `linux/impl-2/lock-drift.txt` and `linux/proof/lock-drift.txt`.

This directory records the SP12-W02 runner proof. Acceptance of B-D04, of the query-pg command, of the query-result adapter, and of the result comparator stays open. The runner consumes those three executables. It does not re-review them. The lane is `trusted_sql`. The report model field is `none`. There is no question-bank rate and no release.

## What the runner does

`sql-runner` takes `--suite PATH --run-dir PATH` in that order. The suite names each case, its role, the ten-field version tuple, k in 1..=8, and the request, binding, and gold files. The configured absolute paths of `query-pg`, `query-result-adapter`, and `result-comparator` are hashed once with `oo_core::canonical::hash_bytes` before any trial. The cap is 256 MiB.

Each trial is a new process for each of the three programs. Stdio is a private regular file created with `create_new`. The run directory is created with `create_dir` only after the suite validates. Gold is a comparator input. The runner report stores the verdict word and the exits. It does not store rows, gold, SQL, or a password.

A trial is `pass` only when `query-pg` exits 0, the summary is `succeeded`, `end_audit_ok` is true, `truncated` is false, and both later programs exit 0 with verdict `match`. A truncated read that the adapter marks complete and the comparator matches is status `fail`, reason `truncated`. Exit 0 is a passed gate with `trial_count` greater than 0. Exit 1 is a valid run whose gate failed, and every planned trial stays in the denominator. Exit 2 is configuration or a report-write failure. `backend_cleanup` is the string `unknown` whenever `query-pg` was spawned.

## Host and images

Linux arm64 Rust `1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL `17.10 (Debian 17.10-1.pgdg12+1)`, image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`.

Postgres container `oo-a231-pg` id `51eeb4f24e8b11d9ebf6153bb6de9b5e95d1ae1fb158874f10a170ba5be308fe`. `pg/up.exit` is 0. Memory 1073741824, NanoCpus 1000000000, privileged false, network `oo-a231-eef831aa-net`, published ports `{}`. CapDrop starts at ALL and adds CHOWN, DAC_OVERRIDE, FOWNER, SETGID, and SETUID. Aliases are `oo-a231-pg` and `oo-a198-pg`. Admin `psql` uses `-w`. `linux/proof/admin/auth-mode.txt` is `local`, so the admin socket connected by local trust. Query requests still name `password_file` and `ca_file`. Those values are files, and they are not argv or environment secrets.

Each rust container had 6 GiB, 1 CPU, all capabilities dropped, no new privileges, and the same private network. Private cargo and target volumes are `oo-a231-eef831aa-cargo` and `oo-a231-eef831aa-target`. The data volume is `oo-a231-eef831aa-pgdata`. Secrets stayed under `/tmp/oo-a231-eef831aa-dba4-4af2-a78b-541bc9e0a231/secrets` and were not copied into this directory.

Recipe origins are `ORIGIN.md`.

## Commands and exits

Pre-edit smoke, container `oo-a231-smoke` id `97e16e3bdb7aae1d5f170b76ebbfa4c34622f2aa93daf3b4aa4483042b1cb0c7`, docker exit 0, `OOMKilled=false`, memory 6442450944, NanoCpus 1000000000, CapDrop ALL. The archive had no `benchmarks/sql-runner` (`runner-absent.exit` 0). The archived `rust-toolchain.toml` was renamed (`toolchain-absent.exit` 0) so the image stayed on 1.94.0. Offline fetch on the fresh cargo volume exited 101. The online fetch exited 0. `oo-core` test `yaml_and_json_numbers_stay_lexical`: 1 passed, 18 filtered, exit 0.

| Attempt | Docker exit | What the files show |
| --- | --- | --- |
| `linux/impl` | 101 | `tests/cli.rs` E0618: a local `request` binding shadowed the helper, so two later calls did not compile. rustfmt exit 0. Clippy exit 101. Container `536b2c54b74ad00a2bfa253e65c95360f00d639f0760e75998eaf4e2c5332172`. |
| `linux/impl-2` | 0 | `cargo test --locked --offline -- --test-threads=1`: 5 CLI tests and 2 SHA-256 tests passed. rustfmt exit 0. Clippy exit 0. Container `25e1505ae2ba20c7fb139ff93e05a95fb0f8260b575c1225b29191149923e686`. |

The impl-2 lock is `benchmarks/sql-runner/Cargo.lock`, 27410 bytes, identical to the copies under `linux/impl-2/` and `linux/proof/`. Pins: serde 1.0.229 checksum `4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba`, serde_json 1.0.151 checksum `c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14`.

Proof container `oo-a231-proof` id `466ef9f0733d43f7fd4e333624877a3612b7af8379add2d609076c0af9fb0bf5`, docker exit 0, `OOMKilled=false`, privileged false, memory 6442450944, NanoCpus 1000000000, CapDrop ALL, network `oo-a231-eef831aa-net`. The container name was removed (`container-removed.txt`). The same 5 CLI tests and 2 SHA-256 tests passed again (`phases/sql-runner-test.exit` 0). `diff -rq` of the archive and the overlay tree, before and after the suites, lists only `benchmarks/sql-runner`, the archived `rust-toolchain.toml`, and `rust-toolchain.toml.repo`. `phases/product.exit` is 0.

`query-pg` was produced by `cargo test -p oo-gateway --test query_pg_cli --no-run` (`phases/norun.exit` 0). `ldd` on that binary exited 0 and contains no `not found`. The frozen live fixture then ran one test, 4 filtered, exit 0, in 2.02s. `live/oracle.txt` records `hidden_exit=6`, `done=ok`, `estimated_rows=3`, `desk_pending=2`, `tuples_before=(3, 0, 0)`, `desk_explicit_rows=3`, `star_rows=3`, and `count_star=3`. That live test is the installer for this proof. It is not a new gateway acceptance.

Hashed ELF digests, identical in `bin-sha256.txt` and in all four reports:

| Binary | Bytes | SHA-256 |
| --- | --- | --- |
| query-pg | 19941408 | `02180ad10825d591851c3e35697af5e9d473a895e28c7467b3f175ff37bd4f60` |
| query-result-adapter | 1160992 | `ee7196c42ade37b041e1714720dc45cc1145c07a8fb8981177ffed4b91bd3154` |
| result-comparator | 1263624 | `c48bd37ed1d7d1ff8725195923b13462776fc373b02533ed6d6ca9b03040f353` |
| sql-runner | 81404120 | recorded in `bin-sizes.txt` only; the report stores the three configured executables |

Total of those four files: 103770144. Each hashed executable is under 268435456. The truncate oracle for this run is 40 (`truncate-bytes.txt`).

## Suite results

Checks compare the runner report with `expect/*.txt`. All four checks exited 0. Suite stderr files are empty. Runner exits are the gate codes.

| Suite | Mode | Runner exit | Questions | Trials | Result |
| --- | --- | --- | --- | --- | --- |
| `live-exact` | frozen_eval | 0 | 1 | 1 | passed, 1 pass |
| `live-fail` | synthetic | 1 | 6 | 6 | 1 pass, 5 fail |
| `live-timeout` | synthetic | 1 | 1 | 1 | 1 timeout |
| `live-noend` | synthetic | 1 | 1 | 1 | 1 fail |

`phases/fail.exit` and `phases/timeout.exit` store those runner exits, 1 and 1. `phases/proof.exit` is 0. `phases/exact.exit` and `phases/noend.exit` are 0.

Every trial has `backend_cleanup` `unknown`. Metadata is `declared`. The tuple snapshot is `fixture-a231` and `as_of` is `2026-10-06`. Those are labels on the report.

| Case | Status | Reason | query | adapter | comparator | verdict | truncated | end_audit_ok |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| exact | pass | match | 0 | 0 | 0 | match | false | true |
| changed | fail | mismatch | 0 | 0 | 1 | mismatch | false | true |
| seal | fail | refused | 6 | | | absent | | |
| hold | fail | refused | 6 | | | absent | | |
| trunc | fail | truncated | 0 | 0 | 0 | match | true | true |
| cmpset | fail | comparator_refused | 0 | 0 | 2 | absent | false | true |
| slow | timeout | timeout | | | | absent | | |
| noend | fail | no_end | 7 | | | absent | | |

Blank exit cells are JSON null. The trunc row matched the comparator and still failed. The noend audit copy is the five bytes `keep` plus a newline. `attempts/noend-occupy.exit` is 0. `end_audit_ok` on that trial is null.

The two tight-filesystem attempts exited 2. Both stderr files are `sql-runner: io`. `attempts/tight-note.txt` says they are recorded and are not the asserted missing-completion trial.

## Database observation

`admin/observe.stdout` lists roles `oo_a198_login`, `oo_fixture_admin`, `oo_pipeline_runtime`, `oo_r_desk`, `oo_r_peek`, and `oo_target_owner`. The mapped catalog rows are namespace `dispatch` and table `ticket` at level `t`. The unmapped relation listed beside them is `dispatch.hold`. Both the seal case and the hold case returned query exit 6.

The timeout lock snapshot was taken before the suite started (`timeout-ready` is after `backends-during-lock`). That snapshot has two backends: pid 161, state active, wait event Timeout, query prefix `SELECT pg_sleep(120);`, and pid 184, the counting query. The slow trial report has query exit null, status `timeout`, and `backend_cleanup` `unknown`. `release.stdout` shows `terminated=t` for pid 161. After release, after the sweep, and at the final count, the backend count is 1 and the remaining row is the counting query. The admin SQL ended that sleep backend. The runner field stays `unknown`.

After each suite, `attempts/*-children.txt` is empty. Those files are the `/proc` scan for the four bench binaries. No `children-after` file was written, so the scan found none to reap. Postgres backends are the separate admin counts above.

## Inventory

`tools/sha256.rs` is the byte copy recorded in `ORIGIN.md`. `linux/inventory/launch.sh` compiles it in the pinned image and writes `crate-sha256.txt` and `SHA256SUMS`. The tool skips paths ending in `/SHA256SUMS` and `/SHA256SUMS.sha256`, then writes the sibling digest of the manifest bytes. `linux/inventory/rerun.sh` runs after the launcher has closed its console, hostconfig, and exit files, and rewrites those manifests. The second stdout stays at `/tmp/oo-a231-eef831aa-dba4-4af2-a78b-541bc9e0a231/hash-again.log`.
