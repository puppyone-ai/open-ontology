# a076 review: P5 parser subset

Date: 2026-10-02. Reviewer session `eef831aa-dba4-4af2-a78b-541bc9e0a076`. No subagents. No commit. The reviewer did not edit the spike, the ADR, the lock, the corpus, or the goldens.

## Decision

`request_changes` for the parser-subset spike only.

This review does not approve P5, Gateway, AT-GW, query safety, R1–R9 as a set, OID/ACL/RLS, or an amd64 build. `syntax_allow` with `oid_acl_rls_proven: false` is a syntax decision. It is not database authorization and it is not an R4 allow.

Two defects are reproduced against commit `efb61d62803073b3f98f5fe1885877824f504f80`:

1. A worker response with `syntax_allow: false`, `code: "ALLOW_SYNTAX"`, and `parser_version: 170007` is returned to the caller as a normal allow, and `executed_sql` is kept. The probe SQL was `DROP TABLE analytics.orders`. The parent did not re-parse it. The fixture never received that statement.
2. The parent does not return while a worker, or a descendant that inherited the stdout pipe, stays alive. After an 8 second external timeout the probe was killed and three workers were still running, with their temp directories still present. A `setsid` grandchild is outside the process group that `kill(-pid, SIGKILL)` targets. On the success path the parent joins the reader before it signals anyone.

`oid_acl_rls_proven: true` from the worker was cleared. The returned code was `INVALID_OUTPUT`, `syntax_allow` was false, and `executed_sql` was empty.

## Identity

| Field | Value |
|---|---|
| Commit | `efb61d62803073b3f98f5fe1885877824f504f80` |
| Parent | `2b68c89934d4ac1a73467ef563b14a895d6c3778` |
| Tree | `da264380bef75579f2691504a653b1c49878978b` |
| Subject | spike: isolate a PG 17.7 parser admission prototype |
| Author | Redux0223 `<171701490+Redux0223@users.noreply.github.com>` |
| ADR | `docs/adr/ADR-P05-parser-prototype.md`, status Proposed |
| Archive | `git archive` of that commit, extracted under the review UUID. `sha256sum -c archive-spike.sha256`: 34 files, 0 failures. |
| Working tree | `git diff efb61d62 -- testing/spikes/p05-pg17-parser docs/adr/ADR-P05-parser-prototype.md` was empty. HEAD at review time was `304fcf9f116bbbabe0f91a2caca5df5949ef476c`. Acceptance used the archive. |

Probe root: `/tmp/oo-a076-81b3f22f-b143-4cf8-b2c8-19110890d30a`. Published worker sources were not modified. The malformed worker is `probes/helper.c` in that temp directory, copied under `raw/probes/`.

## Runtime

One Rust builder and one PostgreSQL fixture. Host `cargo` was not run. No second builder. No `docker prune`, no shared cache, no global git or cargo config.

| Role | Name | Id | Image |
|---|---|---|---|
| Rust | `oo-a076-msrv-81b3f22f` | `0a59e04a4dec9d8a3aedbfd9ee463f0a05fa34b140849cc2a03e5df7956a6eaa` | `rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5` linux/arm64 |
| PostgreSQL | `oo-a076-pg1710-81b3f22f` | `0c849857af671cc48129855ff19ca1d611a241612ed6df0c124706caa1488c70` | `postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb` linux/arm64 |

Both containers were started with `--pull never`, `--init`, `--cap-drop ALL`, and `no-new-privileges`. Inspect of the Rust container: `CapDrop=["ALL"]`, privileged false, network `bridge`, memory 6442450944, pids limit 2048, user `0:0`. `/proc/1/status` inside it: `CapEff`, `CapPrm`, `CapBnd`, `CapInh`, and `CapAmb` are `0000000000000000`, `NoNewPrivs` is 1. The PostgreSQL container used `--network none`, user `999:999`, memory 1073741824, empty port bindings. `rustc 1.94.0 (4a4ef493e 2026-03-02)` aarch64-unknown-linux-gnu.

The author's a065 builder report has cap-drop null. That report is not this run. This inspect and the `/proc/1/status` line are the runtime capability evidence.

`apt-get update` inside the capped container exited 100 (`setgroups`/`seteuid` `EPERM`, and `_apt` could not write lists). `raw/locked-build.log` stops at `apt_update_failed`. `probes/run-locked.sh` then executes `exit 30`. Capabilities were left dropped. Libclang 19.1.7 (`1:19.1.7-3+b1`, plus `libz3-4` `4.13.3-1`) was taken from Debian trixie arm64 debs whose SHA256 matched the trixie Packages index (`raw/debs-manifest.txt`), extracted with `ar` and `tar` into `/opt/clangroot`. `ldd` of that `libclang` reported `ldd_missing=0`. `protoc` was absent.

`cargo fetch --locked` (no `-p`, no `cargo update`) exited 0. Locked tests without `test-hooks`: lib 9, corpus 4, frame 2, doc 0, bins 0. `test_default_exit=0`. With `test-hooks`: the same plus ipc 1. `test_hooks_exit=0`. The first Clippy invocation exited 1 because the component was not installed (`raw/clippy-default.exit`). A login-shell rerun exited 127 because `bash -lc` dropped `PATH` (`raw/clippy-rerun.log`). A later `bash -c` with the image `PATH` installed Clippy and both `cargo clippy --locked --offline --all-targets -- -D warnings` runs exited 0 (`raw/clippy-rerun-2.log`). The 1 and the 127 are kept.

Vendored `pg_config.h`: `PG_VERSION "17.7"`, `PG_VERSION_NUM 170007`, `PG_VERSION_STR "PostgreSQL 17.7 (libpg_query)"`. Crate SHA256 `42a66643c4aa9c929b68b2c9e30d6c4d955bfd1e87e255ce6c5d0c026082436c`, 2042096 bytes, matching the spike provenance pin. `.cargo_vcs_info.json` git sha1 is `0e382742b36ee9592438c6791b91ce14f32f6233`. Smoke `ldd`: `linux-vdso.so.1`, `libgcc_s.so.1`, `libc.so.6`, `ld-linux-aarch64.so.1`.

The fixture loaded `fixtures/pg17-setup.sql` as the postgres superuser. `pg_setup_exit=0`, stderr empty. Server string from `current_setting('server_version')`: `17.10 (Debian 17.10-1.pgdg12+1)`.

Foreign containers were left alone. `oo-a072-pg1710-7c1a0e2b-4f63-4b0a-9c2d-11a072fence01` was still Up at the end of the probes.

## Semantic differential

`raw/decisions.jsonl` has 62 cases from the published `worker-plain` binary (`sha256 0bbba2ff4708b4e77559a97c6f790841ab31f6838c98932e7c1c3152f75a3502`) and from `admit_native`. Every case has the same code and the same `executed_sql` on both paths. Every decision has `oid_acl_rls_proven: false`.

The legal allows were executed on the fixture as `agent_reader` with `statement_timeout=5000`, and compared with an explicit visible-column oracle. All 33 oracles matched, including alias, quoted identifier, CTE, subquery, self-join, `JOIN USING`, `JOIN ON`, `LEFT JOIN`, `UNION`/`UNION ALL`/`EXCEPT`, `count(*)`, `count(*) FILTER`, window `count(*)`, `JSON_TABLE`, and `U&'\0065\0075'` (deparsed to `'eu'`). Star rewrites kept table order: `orders` star became `id, region, amount`; inner `USING (id)` became `orders.id, orders.region, orders.amount, customer.name`; `LEFT JOIN` star kept both sides, including the customer key. `count(*)` stayed `count(*)`. `WITH orders AS (SELECT 9 ...)` did not read `analytics.orders`. `WITH pg_catalog AS (SELECT 1 AS id)` returned that constant. Row counts before and after, as postgres: `big_fact|1`, `customer|2`, `empty_vis|1`, `orders|3`. Unchanged.

`TABLE analytics.orders` was allowed and deparsed to `SELECT orders.id, orders.region, orders.amount FROM analytics.orders`. The fixture returned `1|eu|10`, `2|us|20`, `3|eu|5`. `ORDER BY` on an output alias (`region AS id`) was allowed with the SQL unchanged, and the fixture returned the same rows as that same text.

Hidden and empty columns failed closed: `COLUMN_NOT_ALLOWED`, `NO_VISIBLE_COLUMNS`. `agent_reader` on `SELECT * FROM analytics.orders` and on `SELECT created_at FROM analytics.orders` both returned `42501 permission denied for table orders` (`aclcheck_error`). The parser code and the server SQLSTATE are different strings. Both refuse the hidden column.

Fail-closed codes that matched on native and worker: `UTILITY` (`SET`, `SET LOCAL oo.claims`), `SELECT_INTO`, `ROW_LOCK` (`FOR UPDATE`, `FOR KEY SHARE`), `WRITE_NODE` (writable CTE and nested write), `MULTI_STATEMENT`, `SQL_PARSE_ERROR`, `EMPTY_SQL`, `UNQUALIFIED_RELATION`, `FORBIDDEN_SCHEMA` (`pg_catalog`, quoted `"pg_catalog"`, `information_schema`), `NATURAL_JOIN_DEFERRED`, `LATERAL_DEFERRED`, `WHOLE_ROW_DEFERRED`, `SCOPE_UNRESOLVED` (`JSON_TABLE` `NESTED PATH`). Those deferred codes are scope limits. Empty SQL returns before spawn, so `reaped` and `workdir_removed` stay false and no `oo-p05-*` directory is created.

`NODE_NAMES` and the prost `Node` oneof are the same 268 names, both directions empty. `OTHER_ONEOF_TAGS` are scalar payloads, not node names. An unknown PascalCase node still fails closed. Equal sets mean the walker names every generated variant. They do not mean every variant's children are classified.

## Open scope that this run reproduced

These are already described by the ADR, docs/07 §4, and the a065 report. Reproducing them does not add a product approval and does not make them new defects.

- R9: bare `SELECT *` on `analytics.big_fact` is `R9_STAR_ON_LARGE`. `WHERE true` and `WHERE 1 = 1` are `ALLOW_SYNTAX` and each returned the one fixture row `1|2026-01-01 00:00:00+00`. The wrapper `SELECT s.id, s.ts FROM (SELECT * FROM analytics.big_fact) s` is `ALLOW_SYNTAX` and returned that same row. R9 still keys off a missing `where_clause` and clears the large-table flag for a subquery.
- R4: `SELECT set_config('oo.claims', ...)` and `SELECT pg_read_file('/etc/passwd', 0, 10)` are `ALLOW_SYNTAX`. Neither was sent to the fixture. `SELECT current_setting('server_version')` was sent and returned the 17.10 string. a056 already recorded that utility `SET` can succeed in a database session while this parser rejects the utility and still syntax-allows `set_config`. There is no `pg_proc` whitelist here.
- R5 catalog: the parser returns `FORBIDDEN_SCHEMA` and clears `executed_sql`. `agent_reader` can still `SELECT count(*) FROM pg_catalog.pg_class` (431) and `SELECT count(*) FROM information_schema.tables` (194). Parser deny and database read are different layers.
- Pin: `pg_query` 6.2.0 embeds parser 17.7. The fixture is 17.10. Official release-note pages for 17.8, 17.9, and 17.10 (saved under `raw/relnotes/`) contain no `syntax`, `grammar`, or `JSON_TABLE` hit. The 17.10 "parser" hit is the incremental JSON parser numeric-token crash fix. The 17.10 "SQL command" hits are logical-replication names interpolated without quoting. The 17.8 "parse" hit is server command-line long options. 17.9's change list is WAL replay, `substring` on toasted data, a `LATERAL UNION ALL` nullness fix, two `pg_stat` functions, a PL/pgSQL domain cast, and an hstore crash. `JSON_TABLE` ran on this 17.10 server and returned `1`. That is not a `gram.y` diff, and it is not a claim of grammar parity.

## IPC

Helper modes are symlinks to one binary. The parent under test is the archived `exercise_worker`. Raw log: `raw/ipc-driver.log` (`sha256 e54c3facc54c6c01e1bea95454103cba303cfa28e946773b558673a9ac8bb479`).

| Case | Shell rc | Elapsed | Result |
|---|---|---|---|
| premature exit | 0 | 5 ms | `INVALID_OUTPUT`, reaped, workdir removed |
| spoof pid, starttime, or nonce | 0 | 2–3 ms | `INVALID_OUTPUT` before admit |
| extra frame | 0 | 4 ms | `INVALID_OUTPUT` |
| contradict | 0 | 4 ms | `ALLOW_SYNTAX`, `syntax_allow true`, `executed_sql` `DROP TABLE analytics.orders`, oid false |
| oid true | 0 | 4 ms | `INVALID_OUTPUT`, oid false, SQL empty |
| version 170010 | 0 | 4 ms | `PARSER_MISMATCH`, SQL empty |
| stderr 2 MB then a deny | 0 | 6 ms | `INVALID_OUTPUT` |
| partial frame after ready | 0 | 516 ms | `DEADLINE`, reaped |
| silent sleep, no ready | 0 | 5012 ms | `SPAWN_FAILED`, reaped |
| bad magic `XXXX` then pause | 124 | 8010 ms | no decision; pid 6048 still `S` |
| ready plus 1 MB of `A` | 124 | 8008 ms | no decision; pid 6160 still `S` |
| response length 256 KiB + 1 | 124 | 8011 ms | no decision; pid 6174 still `S` |
| `setsid` holder, no ready | 124 | 8012 ms | descendant 6191, pgid 6191, sid 6191 |
| `setsid` holder after a response | 124 | 8009 ms | descendant 6211, pgid 6211, sid 6211 |
| same-group holder after a response | 124 | 8005 ms | descendant 6231, pgid 6228, sid 5677 |

Foreign `sleep 400` (pid 5712) stayed in state `S` through every case. The driver killed it only after the matrix (`foreign_after=dead`). `pkill -f /probe/helper` returned 1 and did not match the three renamed symlinks, so those workers were still alive when the driver printed `ipc_driver_done`. Nine minutes later `ps` still showed them as ppid 1, state `S`, stdout a live pipe, and these directories still present:

- `/tmp/oo-p05-f21e963a35545519f35cfc0ec4ae9bdf` (bad-magic)
- `/tmp/oo-p05-92194d544628a89de093adff3be3d3fa` (stdout-junk)
- `/tmp/oo-p05-15fcdbce2cfa81052b4dc0be0721287e` (oversize-frame)
- `/tmp/oo-p05-35b133703eea0e70806785b778f7ab41`
- `/tmp/oo-p05-92246e54635cc605a064990c565f7f65`
- `/tmp/oo-p05-e073425dda2b4426cdd251cf9423701d`

`read_frame` returns `INVALID_OUTPUT` as soon as four bytes are not `P05P`, and returns `RESPONSE_CAP` when the length word exceeds 256 KiB, without reading the body (`src/protocol.rs`). The ready-budget and parse-deadline branches call `kill_group` before `reader.join`. The protocol-error branch and the success branch do not. `settle` then calls `child.wait` with `expect_killed` false, so a paused worker is waited on forever. `process_group(0)` plus `kill(-(pid), SIGKILL)` does not reach a child that has called `setsid`. The success path joins the reader before any signal, so a same-group child that holds the write end blocks the parent too. This is the a062 early-`setsid` lesson on this parent: a shell timeout is not a reap, and `reaped: true` was never produced for these cases because the function did not return.

`enforce_parent` (`src/decision.rs`) forces `oid_acl_rls_proven` false and returns `INVALID_OUTPUT` when the worker claimed true. The following branch then runs when the claim was false:

```rust
if self.syntax_allow || self.code == "ALLOW_SYNTAX" {
    self.syntax_allow = version_ok && self.code == "ALLOW_SYNTAX";
```

A false flag with code `ALLOW_SYNTAX` and version 170007 becomes `syntax_allow: true`. `settle` clears `executed_sql` only when `syntax_allow` is false, so the worker's SQL remains. The returned object is the same shape as a real allow, with oid still false.

Path checks, from the same log: a missing path and a directory return `SPAWN_FAILED` in about 1 ms, workdir removed, pid 0. A relative path is resolved against the temp cwd and also returns `SPAWN_FAILED`. A symlink at `/probe/bin/worker-link` was executed (ready accepted, then `DEADLINE` at 513 ms because the helper's mode is `argv[0]`). The parent does not hash the binary and does not require a canonical path.

The inherit probe leaked a Rust `File`. It did not appear in the worker fd table (standard-library files are `CLOEXEC`). The worker's own fds were stdin, stdout, stderr, and the report file it opened. The parent source does not close inherited descriptors above 2. `net-touch`, spawned through `exercise_worker`, read `/etc/passwd`, read `/src/Cargo.toml`, and connected to `1.1.1.1:443`. Worker `/proc/self/status` in that process showed all capability sets zero and `NoNewPrivs` 1, which is the container, not a `capset` in the parent. The published worker source has no socket client and no database URL. The spawn wrapper clears the environment, sets `PATH`, and uses a temp cwd. It does not enter a new network namespace, and a substituted binary can read mounts the container can read. The author's `--network none` smoke was a container around a binary, not a property of `admit_in_worker`.

## Supply and licenses

Independent crates.io plus OSV check of the archived lock, as of `2026-10-01T23:07:52Z`, floor 604800 seconds: 80 registry packages, 0 errors, 0 checksum mismatches, 0 yanked, 0 younger than the floor, 0 missing lock checksums, 0 OSV hits (`raw/supply-summary.json`). `cargo audit` and `cargo deny` were not run. Zero OSV hits are not a full supply-chain approval. The seven-day age check has no appointed owner in this review.

License texts actually compared:

| Spike file | SHA256 | Against the crate |
|---|---|---|
| `bison-2.3-skeleton-exception-excerpt.txt` | `e8ecf341ce94fb7ad19de4a4985374eb821ff84fd4dff943b10a022ca93fc7a3` | Byte-identical to the first 94 lines of vendored `src_backend_parser_gram.c` (53848 lines). |
| `xxhash-bsd-2-clause-header.txt` | `20bf2c9b132fe78c4256abb204159403d987af7a45345f1bdde07ecc094bed7f` | Exact prefix of vendored `vendor/xxhash/xxhash.h`. BSD-2-Clause, Yann Collet 2012–2020. |
| `protobuf-c-bsd-header.txt` | `e8185daf5bcd355692052fbdc990ea2d8acef9b4c3a41a32f189f272667e9d6b` | Same BSD-style block as `vendor/protobuf-c/protobuf-c.h` after normalizing the year. The spike excerpt says 2008–2015. The compiled header says 2008–2018. |
| `pg_query.rs-6.2.0-LICENSE` | `39fac7e7daa7cf3173d96f9e8a167f980b93b90386b83c342de70db3bc7bbb68` | MIT text is in the spike only. No matching file in the crate. |
| `libpg_query-17-6.2.2-LICENSE` | `f9352c20940619a380a5e9f23cebe34d8f797cfbd747fc236a7a3e5fc8ef9b41` | BSD-3-Clause text is in the spike only. The crate contains neither this file nor the bytes `17-6.2.2` or `7be1aed`. |
| `postgresql-17.7-COPYRIGHT` | `5ed3ce5c9373dff7f98b1fae7a6c7ccd98df7d734d46d24c1bcebf1240be8307` | PostgreSQL license text is in the spike only. Individual `.c` files carry their own copyright strings. |

`gram.h` and `src_pl_plpgsql_src_pl_gram.c` also carry GPL-2.0-or-later plus the same special exception. The exception lets a larger work that contains the generated skeleton use other terms, provided that work is not itself a parser generator built from the skeleton. Changing the skeleton, or shipping the skeleton by itself, drops the exception and leaves GPL-2.0-or-later. The exception does not relicense PostgreSQL, `libpg_query`, or this spike as a whole. The crate tarball has no top-level `LICENSE`, `COPYING`, or `COPYRIGHT` for those three recorded bodies. Provenance for the Bison excerpt and the xxhash header is byte-matched. Provenance for the MIT, BSD-3, and PostgreSQL files is the spike copy and its own hash, not a file inside the crate, and the recorded libpg_query commit is not in the tarball.

## Still open

R4, R9, catalog-versus-parser, amd64, a full Gateway and AT-GW run, OID/ACL/RLS, EXPLAIN and memory budgets, a `gram.y` diff from 17.7 to 17.10, worker-path attestation, and a network namespace for the worker. The ADR stays Proposed. A green corpus does not close those gates.

## Cleanup

Text logs and the probe sources were copied into `raw/` before removal. The three hung workers were still state `S` at that moment (pids 6048, 6160, 6174).

`docker rm -f oo-a076-msrv-81b3f22f` exited 0. `docker rm -f oo-a076-pg1710-81b3f22f` exited 0. No `docker prune`. The UUID temp directory was then removed. `docker ps` afterwards listed no `oo-a076-*` container. `oo-a072-pg1710-7c1a0e2b-4f63-4b0a-9c2d-11a072fence01` was still Up. `oo-a072-msrv-997bbcb7-a0e9-4140-8505-e0500a317358` was already Exited and was not removed. Cleanup exits are separate from the test exits above. Records: `raw/cleanup-rust.txt`, `raw/cleanup-pg.txt`, `raw/cleanup-ps.txt`.
