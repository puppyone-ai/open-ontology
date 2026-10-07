# file-import-pg review

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a330`. Independent review of child FILE-D02 / E39 on fixed commit `15558917e2c5be12b74dd1d64fb542b7d05a03af`.

Verdict for this child: **APPROVE_LIMITED**.

Parent SP05-W05 and AT-FILE-06 stay open. This review adds no unit count and no rework count. It accepts no release and no full parent goal.

## Fixed tree

Archive commit `15558917e2c5be12b74dd1d64fb542b7d05a03af`, tree `140fddc79768fd4d1c5b43c4f46ba2acba6cc5bf`. Launch `repo-head.txt` is `53931278e69d723d0ce38961ded897fd6e41d5d3`. `merge-base --is-ancestor` passed inside `launch.sh`. Archive file count `listed=816 extracted=816`. The archive is `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, `crates`, and `tools/file-import-pg` from that commit. The product tree and the root lock were not written.

Author evidence is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a308/SP05/file-import-pg`. That archive was `72dc9e2db841de4de368216bbbe13d738f850ea9`. This review rebuilt from `15558917` and ran the author tests from that archive before copying `tests/review_oracle.rs`.

## Final proof

Directory `linux/proof`. Container `21a07430666a91468e3c32a33502c76846cd26c792cdfc5e8f3b5bd393eec4ca`, removed after the run. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `rustc 1.94.0` host `aarch64-unknown-linux-gnu`. One private PostgreSQL `oo-a330-pg` id `e24b368e74e7ebe5a0c3a8302713aef6a457a6c40a3a04416d0ef6607e142dae` on network `oo-a330-eef831aa-net` (`b3f748d155cacd13b678b9cb828d60863ec7c96a07e97cafae07fdd6517dd12d`, bridge, internal false). Image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, linux/arm64. TLS SAN `DNS:oo-a330-pg`. Published ports `{}`. `up.exit` is 0. Capability drop is `ALL`, with `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, and `SETUID` added back. `pg/inspect.txt` records privileged false.

`hostconfig.txt`: ExitCode 0, OOMKilled false, Privileged false, CapDrop ALL, Memory 6442450944, NanoCpus 1000000000, NetworkMode `oo-a330-eef831aa-net`. Launch flags also set `--user 0:0`, `--security-opt no-new-privileges`, `--memory-swap 6g`, `--pids-limit 4096`, `--pull never`, and `--platform linux/arm64`. Capacity line: `own=0 global=0 host_cargo=0 cap_global=10 cap_own=1 postgres=1`. Client mount is `ca.crt` plus three password files. `docker-run.exit` is 0.

| step | exit |
| --- | --- |
| version | 0 |
| source | 0 |
| sha256-tool | 0 |
| fetch-offline | 0 |
| fetch | 0 |
| fetch-tool-offline | 0 |
| fetch-tool | 0 |
| worker-build | 0 |
| worker-hash | 0 |
| refuse | 0 |
| lock-subset-rustc | 0 |
| lock-subset | 0 (`lock_subset_ok tool_checksums=250`) |
| clippy-add | 0 |
| clippy | 0 |
| live | 0 |
| review-copy | 0 |
| review | 0 |
| clippy-review | 0 |
| lock-stable | 0 |
| docker-run | 0 |

Offline fetch was already satisfied for the workspace and the tool (`fetch.stdout` and `fetch-tool.stdout`). Worker binary SHA-256 `48b6ab508a044a0419ae476f0aacfae41683392abad2002e65dbf8fa56d58054`. Tool lock SHA-256 `9874a7ff5c09b333ab1392829334d648f42340a3ba40d7db167c4690c081e89d`. That lock was not copied back into the product tree. `source-sha256.txt` records the fixed crate files hashed inside the container before the overlay.

`refuse.stdout`: 6 passed in 0.01s (`closed_requests`, `copy_budgets_are_fixed`, `decimal_typmod_and_projection`, `int8_minmax_hash_and_presence`, `path_admission_does_not_open_a_database`, `token_null_and_config_digest`). `live.stdout`: `live_replace_import` 1 passed in 5.03s with `OO_A308_LIVE=1` against `oo-a330-pg`. Clippy before the overlay: `cargo clippy --locked --offline --all-targets -- -D warnings`, exit 0. The vendor crate `sqlx-postgres` printed two unfulfilled `dead_code` expectations. The file-import-pg Clippy exit is 0.

`review.stdout`: `independent_typed_copy` 1 passed in 5.63s with `OO_A330_REVIEW=1`. `clippy-review` exit 0 in 0.29s. `oracle.txt` ends at `oracle_end=1`.

## Independent cases

The review test dropped `oo_a308_file` and the installer roles after the author live and before its own install (`cluster_reset=after_author_live`). Database `oo_a330_file`. Roles `oo_a330_runtime` and `oo_a330_agent`. `pg_roles` count stayed 20 across the CSV import.

CSV `rev.csv`, null policy `empty`, three rows:

- `lo` stores int8 `-9223372036854775808`, numeric `-1.25`, bool `true`, quoted empty note as an empty string, and JSON text `{"k":2}`.
- `hi` stores int8 `9223372036854775807`, numeric `0.00`, bool `false`, note `kept`, and the delimited text `null`.
- `mid` stores SQL NULL for the empty int8 and the empty note, numeric `1.50`, bool `true`, and SQL NULL for the empty JSON cell.

Caller state `awaiting_review`. Stored `_oo.full_import_run.state` `awaiting_publish`. `published` false. `role_grant` false. Shadow `relpersistence` `p`. PUBLIC grantee 0 count `0`. Runtime and agent `SELECT` both return SQLSTATE `42501`. Receipt `snapshot-zero/v1|full-import.snapshot`. Batch id `b0`. Generation `1`. Trigger `full_import_shadow_commit` is `DEFERRABLE INITIALLY DEFERRED`. Guard source contains `state = 'open'`. The import committed, so that deferred check accepted the still-open run before seal.

Replay of the same bytes: `replay_runs=1`, `replay_rows=3`, `run_id` absent, same physical table. Changed bytes on the same receipt: `hash_conflict` / `receipt`, `child_started` true, exit 3, rows and run count unchanged. TSV quoted field `left\tright` round-trips as that text. Token policy: unquoted `\N` is SQL NULL, quoted `\N` is the two-character text, unquoted empty and quoted empty are empty strings. JSONL object `{"k":2}` is JSON text. JSON `null` is SQL NULL. Missing field `payload` returns `missing_field` / `missing_field`, exit 2, with no new run. Hash drift (`id` rewritten from `1` to `9` before start) returns `hash_drift` / `object`, `child_started` false, exit 2. Truncated object `{"id":"cut"` returns `truncated` / `truncated`, exit 4. Complete object `{"id":"done"}` with no newline stores `done` and `awaiting_publish`. Extra CSV field and int8 `9223372036854775808` both return `malformed` / `protocol`, exit 4.

`pg_terminate_backend` on pid 198 returned true. The shadow was absent. `full_import_run` count stayed 5. A fresh import of `k,kept` wrote one row and stored `awaiting_publish`. After each case, `/proc` in the builder had no `oo-file-parser-worker`.

## Preserved review failures

| directory | result |
| --- | --- |
| `linux/proof-failed-1` | docker exit 1. `cargo fetch` rejected `-p`. Author tests did not run. OOMKilled false |
| `linux/proof-failed-2` | refuse 0, live 0, review compile 101. `shadow` was moved before `is_none` |
| `linux/proof-failed-3` | live 101. Author `live.rs:584` saw `replay=true` because attempt 2 left `oo_a308_file` |
| `linux/proof-failed-4` | live 0, clippy-review 0, review 101 at install. Installer roles from the author live made the new database look non-fresh |

Those four directories stay. The passing harness drops the author database and the installer roles after the author live. `pg/reset.txt` records the in-place resets used so the next author live started from `oo_fixture_admin` only.

## Recipe origin

`origin/recipes.txt` records a204 launch and inside, a200 `up.sh` and unchanged `pg-wrap.sh`, a199 manifest tool and launch, and a308 `lock_subset.rs` and `sha256.rs`, with commit, path, and SHA-256. Deltas are a330 names, a 6GiB 1-CPU builder, the `15558917` archive including the tool, author tests before the overlay, client secrets only, no product lock write-back, and a 6GiB 1-CPU manifest container.
