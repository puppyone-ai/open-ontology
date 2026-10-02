# a065 P5 parser subset

Parent: `2b68c89934d4ac1a73467ef563b14a895d6c3778` (`docs: dispatch the bounded Rust query SDK with exclusive dependency ownership`). This report is inside the owned-file commit, so that commit's hash is the session handoff.

Status: Proposed subset. SP06 and P5 are not accepted. R1–R9 are not closed. The a056 permission spike does not, by itself, prevent Agent `SET` SQL. This parser rejects utility `SET` before send. `SELECT set_config(...)` remains `ALLOW_SYNTAX` and was not executed.

## Versions

- Rust crate `pg_query` 6.2.0, checksum `42a66643c4aa9c929b68b2c9e30d6c4d955bfd1e87e255ce6c5d0c026082436c`, git `0e382742b36ee9592438c6791b91ce14f32f6233`.
- Vendored libpg_query `17-6.2.2` commit `7be1aed1f1f968a36cf541319f71e845850f0381`. Embedded `PG_VERSION` 17.7, `PG_VERSION_NUM` 170007.
- Fixture server `17.10 (Debian 17.10-1.pgdg12+1)`, `server_version_num` 170010, `pg_is_in_recovery` false.
- `pg_query` 6.2.1 (2026-09-30) still embeds `PG_VERSION_NUM` 170007 and is under the age floor. Not used. Default PostgreSQL was not changed.
- Builder image `rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`.
- Fixture image `postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, linux/arm64.
- Host checks: Homebrew `rustc 1.98.1 (48a229cea 2026-09-01)`, `cargo 1.98.1`. Not the MSRV proof.
- `sqlparser` is not a dependency.

## APIs

Library `oo_p05_pg17_parser`:

- `admit_native(sql, policy) -> Decision` runs inside the worker.
- `admit_in_worker(worker_bin, sql, policy) -> Decision` is the parent path.
- `exercise_worker(worker_bin, kind, body) -> Decision` is the fault-injection path.
- `Policy::parse` requires `schema.name` keys. Visible columns are an in-order subsequence of `columns`.
- `run_worker() -> i32`. Bins: `oo-p05-parser-worker`, `oo-p05-pg17-parser-smoke`.

Frame: magic `P05P`, version u16 `1`, kind u16, body length u32, then body. Kinds: ready 1, admit 2, response 3. Test-hooks only: spin 10, crash 11, garbage 12, fake-allow 13. SQL max 65536. Frame body max 256 KiB. Nodes max 10000. Depth max 128. Parse deadline 500 ms after ready. Spawn bound 5 s.

`Decision` fields: `syntax_allow`, `code`, `parser_version`, `node_count`, `max_depth`, `original_projection_star`, `executed_sql`, `detail`, `oid_acl_rls_proven`, `nonce`, `pid`, `reaped`, `workdir_removed`, `starttime_matched`. Allow requires code `ALLOW_SYNTAX`, parser version 170007, and `oid_acl_rls_proven` false. Deny clears `executed_sql`.

The worker process group is 0, env is cleared, `PATH` is `/usr/bin:/bin`, cwd is an empty `oo-p05-{nonce}` temp directory, and the only argv is the nonce. Deadline and identity failure SIGKILL the group, join the reader after the kill, wait, require `kill(pid, 0)` to return `ESRCH`, and delete the workdir. On Linux, ready-frame `starttime` must match `/proc/<pid>/stat` field 22 or the parent returns `INVALID_OUTPUT` before admit. The Linux corpus allows reached `ALLOW_SYNTAX`, so that check passed. The flag itself was not printed.

## Commands and exits

Host, in `/tmp/p05-check`, `LIBCLANG_PATH=/Library/Developer/CommandLineTools/usr/lib`, repo lock not rewritten by this cargo:

```
cargo test --features test-hooks --offline -- --test-threads=8
cargo clippy --all-targets --offline -- -D warnings
cargo clippy --all-targets --offline --features test-hooks -- -D warnings
```

Exit 0. Lib tests 9 passed. Corpus 4 passed. Frame 2 passed. IPC 1 passed. Doc-tests 0. Clippy finished with no warnings.

Linux builder `oo-a065-p5-rust194-locked` id `dd6cd719fdde69a3b3dd2ca34fdb279553dffbc7352bef25d654e0b5ef72efa4`. Started 2026-10-01T23:56:46.287897634Z, finished 2026-10-01T23:57:35.044128337Z, exit 0, OOMKilled false. Limits: memory 6442450944, memory-swap 6442450944, nano CPUs 3000000000, pids 1024, privileged false, network bridge, cap-drop null, `no-new-privileges`. Mount only the spike at `/src`. `CARGO_TARGET_DIR=/tmp/p05-target`. cgroup inside the script: `memory_max=6442450944`, `cpu_max=300000 100000`, `pids_max=1024`. `protoc_present=no`.

```
cargo update -p cc --precise 1.4.7
cargo update -p find-msvc-tools --precise 0.1.13
cargo update -p lazy_static --precise 1.5.0
```

Log lines: `Downgrading cc v1.5.1 -> v1.4.7`, `Downgrading find-msvc-tools v0.1.14 -> v0.1.13`, `Downgrading lazy_static v1.5.1 -> v1.5.0`. Then `cargo fetch --locked`, `cargo test --locked --offline` (9, 0, 0, 4, 2, doc 0, all ok; ipc absent without the feature), `cargo test --locked --offline --features test-hooks` (same plus ipc 1 ok), both Clippy commands, `locked_phase_exit=0`. Full log: `raw/locked-build.log`.

Age recheck of the written lock against crates.io at the same `as_of`, floor 604800s: 80 registry packages, 0 young, 0 yanked, 0 checksum mismatches. OSV `POST https://api.osv.dev/v1/querybatch` for those 80: 0 vulnerability hits. `raw/age-check.json`, `raw/osv-batch.json`.

PG fixture `oo-a065-p5-pg1710` id `213bfe6c0915cad247c668324744f2eb319e09b242d0f88034adfde54d708702`. `--network none`, ports `{}`, privileged false, memory 1073741824, swap 1073741824, nano CPUs 1000000000, pids 256, `no-new-privileges`. Setup SQL exit 0 (`CREATE` / `INSERT` / `GRANT` as printed by `psql`). No agent `SET`, `COPY`, `DELETE`, `MERGE`, `EXPLAIN`, `DO`, `CALL`, `PREPARE`, or `set_config`. `agent_reader` is `LOGIN` only so the probe can connect without sending `SET`. Results: `raw/pg-fixture.jsonl`.

Runtime probe of the Linux smoke binary copied from the builder, then:

```
docker run --rm --network none --cap-drop ALL --memory 256m --memory-swap 256m --pids-limit 64 \
  --security-opt no-new-privileges --entrypoint /smoke \
  rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5
```

`ldd` showed only `libgcc_s`, `libc`, and the loader. Smoke exit 0: `smoke_ok=1`, `parser_tree_version=170007`, `deparsed=SELECT 1`, `stmt_count=1`. This probe is not a second Rust builder.

## License inventory

See `testing/spikes/p05-pg17-parser/DEPENDENCY-PROVENANCE.md` and `licenses/`. Bison 2.3 skeleton excerpt is GPL-2.0-or-later with the Bison special exception. That is not a blanket GPL exception. OSV returned no advisories for the locked graph on this date. MSRV proof is the 1.94 container. `pg_query` publishes no `rust-version`. `bindgen` 0.66.1 needs libclang at build time and is not linked at runtime.

## Positive results

Corpus and IPC on the host and on Linux 1.94 matched `corpus/expected.json`, including legal selects, `count(*)`, star rewrite fragments, utility and write denies, worker deadline, crash, garbage, version mismatch, and a forged OID claim cleared to `oid_acl_rls_proven` false / `INVALID_OUTPUT`.

Fixture rows, parser version 170007, `oid_acl_rls_proven` false:

| id | server | parser | deparsed server |
|---|---|---|---|
| legal-filter | rc 0, rows `1\|eu\|10` and `3\|eu\|5` | `ALLOW_SYNTAX` | same rows |
| raw-star-orders | `42501` permission denied for table orders | `ALLOW_SYNTAX`, star true, SQL `SELECT o.id, o.region, o.amount ...` | rc 0, same two rows |
| count-star | rc 0, `2` | `ALLOW_SYNTAX`, star false, `SELECT count(*) ...` | `2` |
| join-using-star | `42501` | `ALLOW_SYNTAX`, `orders.id, orders.region, orders.amount, customer.name` | `1\|eu\|10\|ada` |
| customer-visible | `1\|ada`, `2\|bob` | `ALLOW_SYNTAX` | same |
| json-table | `1` | `ALLOW_SYNTAX`, deparse keeps `JSON_TABLE` and `'$[*]'` | `1` |
| big-visible | one row | `ALLOW_SYNTAX` | same row |
| hidden-created-at, customer-email, empty-star, empty-column | `42501` permission denied for table | `COLUMN_NOT_ALLOWED`, `COLUMN_NOT_ALLOWED`, `NO_VISIBLE_COLUMNS`, `COLUMN_NOT_ALLOWED` | not executed |
| unqualified | `42P01` relation "orders" does not exist | `UNQUALIFIED_RELATION` | not executed |

Column privileges for `agent_reader`: orders `id, region, amount` true and `created_at` false; customer `id, name` true and `email` false; big_fact `id, ts` true and `amount` false; empty_vis `id` and `secret` false.

## Negative results and open P5 work

- `where-true-large`: parser `ALLOW_SYNTAX` with the original star set. Deparsed `SELECT b.id, b.ts FROM analytics.big_fact b WHERE true` returned the row. Raw star was `42501` because `amount` is not granted. R9 did not fire. `WHERE true` is an open hole.
- `r9-wrap`: original and deparsed SQL both returned the row. A subquery that projects the visible columns clears the large-table flag.
- `catalog`: parser `FORBIDDEN_SCHEMA`. Server rc 0 and 431 `relname` values. This role can read `pg_catalog`. The parser has to deny before send. The fixture does not prove R5 at the database.
- R4 OID, operator, and cast binding is not implemented. No fabricated OIDs. `set-config-call-not-oid-proof` is an allow in the corpus and was not sent to PostgreSQL.
- Deferred denies present in the corpus and the walker: `NATURAL_JOIN_DEFERRED`, `JOIN_USING_OUTER_DEFERRED`, `LATERAL_DEFERRED`, `WHOLE_ROW_DEFERRED`, `RECURSIVE_BINDING_DEFERRED`. Code also returns `SCOPE_UNRESOLVED` for a `JOIN USING` alias and for nested `JSON_TABLE` columns. Those two were not separate fixture statements.
- AST depth 128 is unit-tested on synthetic JSON (`depth_budget_is_one_past_128`). Real SQL of 20 nested selects is `SQL_PARSE_ERROR` because the vendored parser stops first (15 nested scalar selects parse at depth 48; 16 fails parse). Node budget `SELECT 1,1,...` of 6000 items is `AST_TOO_MANY_NODES` in the corpus.
- Server text for a hidden column is `permission denied for table`, SQLSTATE `42501`, not the spike code `COLUMN_NOT_ALLOWED`.
- No RLS, no authenticated policy, no EXPLAIN cost budget, no product role, no Gateway, no x86_64 run. Prost will not show a field the JSON walk never receives if the version stays 170007.
- Full dual-arch matrix was not run. Native arm64 compiled and the smoke binary ran. That does not sign the matrix.

## Cleanup

Removed only the earlier a065 smoke container `83afa18a6cac` before the locked builder. Runtime probe containers used `--rm`. After this report was written, the session removes `oo-a065-p5-rust194-locked` and `oo-a065-p5-pg1710` only. Other owners' containers stay.
