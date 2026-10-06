# Verdict

APPROVE_LIMITED

Session `eef831aa-dba4-4af2-a78b-541bc9e0a244`. This verdict covers the trusted SQL runner at fixed commit `8f533140a2823568c5f375190f8708934cd413fa` only. The lane is `trusted_sql`. The report model field is `none` and the harness field is `sql-runner`.

The runner at that commit drives one new `query-pg`, then the query-result adapter, then the result comparator, for each trial. This review compiled those three programs from the same sealed archive and ran fresh hand-written suites through `sql-runner` on a private TLS Postgres. It does not accept the query-pg command, the adapter, the comparator, trial statistics, a question bank, a real frozen database, an agent session, a customer run, or a release. It does not update a ledger or an acceptance count. The a242 benchmark-tools review is a separate review and is not repeated here.

## Frozen commit

Commit `8f533140a2823568c5f375190f8708934cd413fa`, parent `fdf6d99a53d0d07a826450d321ff201d8118c0a9`, tree `94005c335ef758877b545fee4f4f258123e4e41e`, message `sql-runner: add the trusted SQL trial runner`, authored 2026-10-06 07:54:41 +0800. That commit is an ancestor of repository HEAD `1715d499578bcb1c36697ed15bca75780bd690d3`. `source/repo-head-at-extract.txt` records `64153894d3b0b0e184202e7b6054d743578f71ed`. `linux/proof/repo-head.txt` records `1715d499578bcb1c36697ed15bca75780bd690d3`. The build mounted a git archive of `8f533140`, not the dirty worktree.

`benchmarks/sql-runner` is already inside that commit. The proof did not overlay a worktree copy. `linux/proof/tree-diff.txt` and `linux/proof/tree-diff-after.txt` each contain only:

```
Only in /archive: rust-toolchain.toml
Only in /src: rust-toolchain.toml.repo
```

The archived `rust-toolchain.toml` asks for a newer channel. It was renamed on the archive copy before cargo so `RUSTUP_TOOLCHAIN=1.94.0` stayed on the image. `phases/source-check.exit` and `phases/source-after.exit` are 0. `source-sha256.check` lists every sealed `benchmarks/sql-runner` file as OK. `cargo update` was not invoked (`cargo-update-invoked.txt` is `no`). `lock-drift.txt` is empty. The root lock pins serde 1.0.229 checksum `4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba` and serde_json 1.0.151 checksum `c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14` (`linux/smoke/lock-pins.txt`). No product, manifest, or lock file was edited.

## What ran

Smoke ran before the proof, on the sealed archive. Container `oo-a244-smoke` id `970d607473b1fca19cae057be89e99e157abeb55810c4189db145e7ddf890099`. `docker-run.exit` 0, `test.exit` 0. Host config: privileged false, network `oo-a244-eef831aa-net`, CapDrop ALL, memory 6442450944, NanoCpus 1000000000. Capacity at start: `own=0 global=0 host_cargo=0`. `runner-present.exit` is 0. `toolchain-absent.exit` is 0. rustc reports `1.94.0`. The sql-runner tests are 5 passed in 6.94s and 2 SHA-256 tests passed. The container was removed (`container-removed.txt` is `oo-a244-smoke`).

The successful proof is container `oo-a244-proof` id `67afd1256a6f116b7e6c9d9643c82e5d28ca5a72ba0f9fd8936dc3f7ce132009`. `docker-run.exit` 0. Host config: ExitCode 0, OOMKilled false, privileged false, network `oo-a244-eef831aa-net`, CapDrop ALL, memory 6442450944, NanoCpus 1000000000. Capacity at start: `own=0 global=1 host_cargo=0`. The one foreign rust-builder was left alone. The same 5 CLI tests passed in 7.03s and the 2 SHA-256 tests passed (`phases/sql-runner-test.exit` 0). Clippy on the sealed manifest, `--locked --offline --all-targets -- -D warnings`, exited 0. The image did not ship cargo-clippy (`clippy-version.stderr`). `rustup component add clippy` inside that container exited 0, then clippy printed `Finished dev profile [unoptimized] target(s) in 0.04s` and wrote no warning. `phases/clippy.exit` is 0. `phases/product.exit` is 0. `phases/proof.exit` is 0. The container was removed.

`query-pg` came from `cargo test -p oo-gateway --test query_pg_cli --no-run` (`phases/norun.exit` 0, binary path in `query-pg-test-exe.txt`). `ldd` without `LD_LIBRARY_PATH` lists linux-vdso, libgcc_s, libm, libc, and ld-linux (`query-pg-ldd.txt`, `phases/ldd.exit` 0). The frozen live fixture `live_query_pg_against_private_postgres` then ran with `OO_A198_LIVE=1` (`phases/live.exit` 0). Safe facts are in `attempts/oracle-facts.txt`: `estimated_rows=3`, `desk_pending=2`, `desk_explicit_rows=3`, `star_columns=lane,rank,stamp,note,open`, `star_rows=3`, `count_star=3`, `peek_rows=3`, `truncate_bytes=40`, `truncate_rows=1`, hidden, write, and sleep exits 6 with audits absent, `done=ok`. `truncate-bytes.txt` is `40`. `fill.rs` substituted that measured value. `live/desk-explicit.stdout` is the marker `withheld` and `bytes=937`. The kept audit files are admit and complete records with hashes and counts. This live test installed the fixture the runner then used. Its exit 0 is not a new gateway acceptance.

The database is `oo-a244-pg` id `ffab91fea754283c7e54b6bba620cc92eb035ee50cb58bf5eda7da1674e0b9b3`, image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `postgres (PostgreSQL) 17.10 (Debian 17.10-1.pgdg12+1)`, `pg/up.exit` 0, started `2026-10-06T01:16:19.305063174Z`. Memory 1073741824, NanoCpus 1000000000, privileged false, network `oo-a244-eef831aa-net`, published ports `{"5432/tcp":null}`, CapDrop ALL, CapAdd CHOWN, DAC_OVERRIDE, FOWNER, SETGID, SETUID. Certificate subject `CN=oo-a244-pg`, SAN `DNS:oo-a244-pg, DNS:oo-a198-pg`. The alias `oo-a198-pg` is how the frozen fixture's hardcoded host reaches this database. `admin/auth-mode.txt` is `local`. Admin `psql` used `-w`. Query requests name `password_file` and `ca_file`. Those paths were not copied into this directory.

Rust image for smoke, proof, and inventory: `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64. Both images were already local. Nothing was pulled. Each rust container had 6 GiB, 1 CPU, all capabilities dropped, no new privileges, `HOME=/tmp`, and the private network. Volumes are `oo-a244-eef831aa-cargo`, `oo-a244-eef831aa-target`, and `oo-a244-eef831aa-pgdata`. Secrets stayed under `/tmp/oo-a244-eef831aa-dba4-4af2-a78b-541bc9e0a244/secrets`.

Recipe origins are `ORIGIN.md`.

## Executables

Hashed once with the runner before the run directory was created. The same three digests appear in every suite report and in `bin-sha256.txt`.

| Binary | Bytes | SHA-256 |
| --- | --- | --- |
| query-pg | 19941472 | `8dda46d6d887b02e2568bba4d9f6d9f6a51548b3494e889999f9b9e275e8f92f` |
| query-result-adapter | 1160992 | `ee7196c42ade37b041e1714720dc45cc1145c07a8fb8981177ffed4b91bd3154` |
| result-comparator | 1263624 | `c48bd37ed1d7d1ff8725195923b13462776fc373b02533ed6d6ca9b03040f353` |
| sql-runner | 1518984 | `fb8d535c8ea26ba679054eebbab20452907fcbcc7a5598e13404e224008dcea0` |

Total 23885072. Each file is under 268435456. The sql-runner digest is recorded in `bin-sha256.txt`. The report stores the three configured executables. Adapter and comparator digests match the a231 proof's recorded digests. The query-pg digest is the binary built from this archive. `CARGO_PROFILE_DEV_DEBUG=0` stayed set for the proof build.

## Suite results

Hand gold and requests are under `hand/`. Expects are under `expect/`. Checks compare each runner report with its expect. All five `*.check.exit` files are 0. Each report's stdout bytes equal its `report.json`. Each suite stderr is empty. Runner exits are the gate codes. `not_run_count` is 0 on every report. `metadata` is `declared`. Tuple `snapshot` `fixture-a244` and `as_of` `2026-10-06` are labels on the report. `question_count` is the case count.

| Suite | Mode | Runner exit | Questions | Trials | Gate |
| --- | --- | --- | --- | --- | --- |
| pass | frozen_eval | 0 | 1 | 2 | passed, 2 pass |
| fail | synthetic | 1 | 4 | 4 | 4 fail |
| peek | synthetic | 1 | 2 | 2 | 1 pass, 1 fail |
| timeout | synthetic | 1 | 1 | 1 | 1 timeout |
| noend | synthetic | 1 | 1 | 1 | 1 fail |

`phases/pass.exit` is 0. `phases/fail.exit`, `phases/peek.exit`, and `phases/timeout.exit` are 1, the runner exits. `phases/noend.exit` is 0 because the expect matched. `reports/noend.exit` is 1, the runner exit. The first missing-completion attempt met the expect. `attempts/noend-occupy.exit` is 0 and `attempts/noend-audit.jsonl` is the five bytes `keep` plus a newline. There is no retry directory.

| Case | Status | Reason | query | adapter | comparator | verdict | truncated | end_audit_ok | backend_cleanup |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| exact 0 | pass | match | 0 | 0 | 0 | match | false | true | unknown |
| exact 1 | pass | match | 0 | 0 | 0 | match | false | true | unknown |
| changed | fail | mismatch | 0 | 0 | 1 | mismatch | false | true | unknown |
| seal | fail | refused | 6 | null | null | absent | null | null | unknown |
| hold | fail | refused | 6 | null | null | absent | null | null | unknown |
| trunc | fail | truncated | 0 | 0 | 0 | match | true | true | unknown |
| notes | pass | match | 0 | 0 | 0 | match | false | true | unknown |
| hidden | fail | refused | 6 | null | null | absent | null | null | unknown |
| slow | timeout | timeout | null | null | null | absent | null | null | unknown |
| noend | fail | no_end | 7 | null | null | absent | null | null | unknown |

The pass case planned k=2. Both trials passed and stayed in `trial_count`. `attempts/facts.txt` records `pass_exact_requests_differ=yes`. The fail suite planned 4 trials and kept 4 failures. The peek suite is a second suite because the role must match the identity role. Notes matched. `SELECT lane` under the peek role stopped at query exit 6. The trunc trial is a matched truncated prefix: comparator verdict `match`, comparator exit 0, `truncated` true, status `fail`, reason `truncated`. `facts.txt` records `trunc_comparator_verdict=match`, `trunc_comparator_has_cell=no`, `trunc_comparator_bytes=90`.

`facts.txt` also records `requests_scanned=10`, `requests_with_secret=0`, `requests_with_gold_path=0`, `noend_audit=keep`. Gold files remain under `hand/` and `linux/proof/built/` because they are the comparator input. `built/request-poison.json` contains a gold path on purpose and was refused before a run directory existed.

Child scans for live, pass, fail, peek, timeout, noend, bad-hash, and gold-poison are empty. Each scan ran after that command returned and before any host kill. No `child-leaks.txt` was written.

## Bounds

| Bound | Runner exit | stdout | stderr | run dir |
| --- | --- | --- | --- | --- |
| bad hash | 2 | empty | `sql-runner: hash /executables/query_pg/sha256` | absent |
| gold poison | 2 | empty | `sql-runner: gold /cases/0/request_file` | absent |

`phases/bad-hash.exit` and `phases/gold-poison.exit` are 0. Those are the harness checks. The runner exits are `attempts/bad-hash.exit` and `attempts/gold-poison.exit`.

## Backend cleanup

`backend_cleanup` is `unknown` on every trial whose `query-pg` was spawned, including the timeout kill and the refusals. That word means the runner did not observe a backend release. It is the contract value. It does not establish that a database session closed.

`host-coord.log` times, minute resolution on the proof clock: 01:22:21 hold, 01:22:30 timeout-start, 01:22:32 timeout-ready, 01:22:36 after-runner, 01:22:38 timeout-release. `backends-during-lock.stdout` shows 2 backends: pid 162 active on `SELECT pg_sleep(120);` and the snapshot query. `backends-after-runner.stdout` was taken after the runner had returned and before admin release. It shows 3 backends. Pid 162 was still in `pg_sleep`. Pid 180 was active, `wait_event_type` Lock, client backend. The query prefix on that row was physical SQL and was replaced in the file with `[withheld physical sql]` before this inventory. `release.stdout` shows admin `pg_terminate_backend` of pid 162 only, the sleep. `sweep.stdout` is 0 rows. `backends-after-release.stdout` shows only the snapshot connection, pid 207. The timeout report already had `query_exit` null, reason `timeout`, and `backend_cleanup` `unknown` while pid 180 was still waiting on Lock. The later absence of that backend followed the admin terminate of the lock holder.

## Preserved harness failure

`linux/proof-1/` is the first proof launch. Docker exit 1. Container id `d95bb048b05930f0191f6503cf18287379ba59c7913d769132a383b5b93b68d4`. Host config ExitCode 1, OOMKilled false. Clippy and the sql-runner tests had already exited 0. `tools/fill.rs` shadowed the `exes` function, so the later call failed with E0618. No libclang install and no live suite ran (`WHY.txt`). The binding was renamed in the harness only. The product tree was not edited. The successful proof did not overwrite `linux/proof-1/`.

Comparator name `set` and the tight tmpfs attempts from the author proof were not repeated. The sealed unit tests already cover a comparator refusal, and this review had no new failure that required those attempts.

## Inventory

`linux/inventory/launch.sh` compiles `tools/sha256.rs` in container `oo-a244-hash` and writes `SHA256SUMS` for this evidence directory. `linux/inventory/rerun.sh` rewrites that manifest after the first pass has copied its console and host config into the tree. The second-pass stdout and host config stay under `/tmp/oo-a244-eef831aa-dba4-4af2-a78b-541bc9e0a244`. The hasher skips paths ending in `/SHA256SUMS` and `/SHA256SUMS.sha256`. Host `shasum` is not this inventory. The live workspace crate is not mounted. The sealed runner sources are `source/blobs/benchmarks/sql-runner/`.
