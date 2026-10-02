# ADR-P05-parser-prototype

| Field | Value |
|---|---|
| Status | Proposed |
| Date | 2026-10-02 |
| Supersedes | none |
| Superseded by | none |

Prototype record for the SP01 P5 parser slice. It does not continue ADR-001 through ADR-016 in `docs/02-technology-stack.md`, and landing it does not amend that file or the Spec. Reviewer columns stay unassigned. Proposed is not Accepted. SP06 and P5 stay unsigned.

## Background

REQ-GW-03 and `docs/07-gateway.md` §4 require one top-level `SelectStmt`, a recursive write rejection, no `SELECT INTO` or row lock, utility rejection including `SET`, a 64 KiB SQL cap, an AST node cap, depth 128, and a 500 ms parser deadline. Star expansion is scope-aware, leaves `count(*)` alone, preserves output order, and R9 is judged on the original projection. `docs/17-rust-implementation.md` §6 puts the native parser in a killable process with a length limit before parse. Parse failure stays `SQL_PARSE_ERROR`. `sqlparser` is not an allow path. Plan P5 / SP06-W02 is the full R1–R9, OID, and DB-enforcement slice. This spike is the isolated parser subset.

a056 (`docs/adr/ADR-P06-P13-prototype.md`) showed `SET LOCAL oo.claims` is not stopped by revoking `set_config`, and that the same transaction then saw the new claim. This spike rejects utility `SET`, `SET ROLE`, `SET SESSION AUTHORIZATION`, and `SET LOCAL` before any database send. That is a syntax gate. It does not re-prove the a056 session, and a `SELECT set_config(...)` allow is not an OID or ACL result.

The code lives only in `testing/spikes/p05-pg17-parser/`. The original measurement is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a065/SP01/P5/`. The a080 IPC correction, against a076 `request_changes` on `efb61d62803073b3f98f5fe1885877824f504f80`, is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a080/SP01/P5-ipc-correction/`.

## Decision

Pin Rust `pg_query` 6.2.0. The vendored parser is PostgreSQL 17.7 (`PG_VERSION_NUM` 170007), libpg_query `17-6.2.2` commit `7be1aed1f1f968a36cf541319f71e845850f0381`. The fixture server is PostgreSQL 17.10 (`server_version_num` 170010). Same major, different patch. No crates.io `pg_query` release embeds 17.10. `pg_query` 6.2.1 is under the seven-day floor and still embeds 17.7. The product default stays PostgreSQL 17. A parser/server major mismatch, an unknown node, or a parser version other than 170007 on an allow cannot return allow.

Native parse and deparse run in a separate worker process. The parent frames version and length before spawn, tracks the child pid and nonce, and starts the 500 ms deadline after the ready frame. Deadline, abnormal exit, invalid output, parser mismatch, and unsupported syntax return deny. `oid_acl_rls_proven` is forced false. A worker that claims true becomes `INVALID_OUTPUT`.

a080 changes the parent screen and the process lifecycle. The parent rejects a contradictory allow shape and clears `executed_sql`. `syntax_allow == false` together with code `ALLOW_SYNTAX` stays false and becomes `INVALID_OUTPUT`. It is not rewritten to true, and a wrong version on that shape is not turned into `PARSER_MISMATCH`. A deny that still carries SQL, an unknown code, an unknown JSON field, or an oversize allow payload is `INVALID_OUTPUT` with SQL cleared. The parent does not call the native parser to repair worker output. `syntax_allow` is a static-syntax mark. Gateway must bind and authorize before any execute. This spike is not that authorization.

The trusted image is the caller-supplied SHA-256 of a regular file opened with `O_NOFOLLOW` on every path component, including ancestors. Those bytes are copied into a sealed memfd and exec'd via `/proc/self/fd`. A symlink, a relative path, a hash mismatch, or an empty or oversize image is `UNTRUSTED_WORKER` and is not executed. A missing path or a directory is `SPAWN_FAILED`. The hash is not a signature. A caller who hashes an untrusted path and passes that digest has not created an external root. Replacing the path after the open does not change the bytes already copied. This does not prove that `memfd_noexec` will allow the exec, and it does not prove anything about a file the caller never pinned. The nonce, pid, and starttime bind the session to the process that was spawned. They do not authenticate the worker's output. The official worker's test-hook kinds and the malformed fault worker run only behind the `test-hooks` feature. A default build rejects `allow-fault`. The self-hash used by the test hook is not an external pin, and that hook cannot return `syntax_allow`.

Every terminal branch closes the worker's descriptors and bounds the wait. There is no `reader.join` and no unbounded `child.wait`. Readiness is 5 s, parse is 500 ms after ready, and cleanup is 500 ms. A bad magic, stdout junk, an oversized advertised length, a malformed response, and an early exit return inside the cleanup ceiling. A response with the stdout pipe still held, a same-group child, and a `setsid` grandchild are killed by a per-invocation supervisor. That supervisor sets `PR_SET_CHILD_SUBREAPER` on itself only. It records pid, starttime, ppid, pgid, and sid, SIGKILLs those descendants, and reaps them with `waitpid`. The parent checks that proof and its own pre-signal snapshot: a pid that still has the same starttime means the proof failed. `reaped` is true only when the supervisor exited, the proof holds, the workdir was removed, and the workdir byte sum is within budget. A scan error is not an empty tree. If `prctl` fails, the code is `CONTAINMENT_UNAVAILABLE` and `reaped` stays false. If the supervisor dies before the reap, orphans go to pid 1 and the result stays not proven. This builder's capability set cannot create a child cgroup or a user/pid namespace (`unshare(CLONE_NEWUSER)` returns `EPERM`, the cgroup filesystem is read-only). Those are not claimed. Process-global subreaper mutation, `pkill`, and proc-name matching are not the proof. `RLIMIT_NPROC` is not enforced for uid 0 and is not used. `RLIMIT_FSIZE`, `RLIMIT_CPU`, `RLIMIT_AS`, `RLIMIT_NOFILE`, and `RLIMIT_CORE` are set in the worker child.

The worker's environment is cleared and its cwd is the empty workdir. That is not network isolation. In this builder the worker stays in the parent's network namespace, can open `/etc/passwd`, and can create a socket. It does not receive the parent's extra file descriptors. No gVisor/`runsc` result is claimed.

Admission allows exactly one top-level `SelectStmt`. Write nodes, `SELECT INTO`, row locking, and other utility statements are denied at any CTE, subquery, or set-operation depth that the walker reaches. The star rewriter expands a projection star to the synthetic visible-column policy, then deparses, reparses, and scans again. `count(*)` stays an aggregate star.

## What this run measured

Linux arm64 Rust 1.94.0, image digest `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, container `oo-a065-p5-rust194-locked`, exit 0. Locked tests and Clippy `-D warnings` passed for the default features and for `test-hooks`. `protoc` was absent. Host Homebrew rustc 1.98.1 also passed the same suite. The host compiler is not the MSRV proof.

The PG 17.10 fixture, image digest `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, network `none`, no published port, ran read-only statements as `agent_reader`. Column grants match the synthetic policy. Raw `SELECT *` on `analytics.orders` returned SQLSTATE `42501`. The deparsed visible list `o.id, o.region, o.amount` returned the same two `eu` rows as the explicit query. `count(*)` stayed `count(*)` and returned 2. `JOIN ... USING (id)` deparsed to `orders.id, orders.region, orders.amount, customer.name` and returned `1|eu|10|ada`. `JSON_TABLE` parsed, deparsed, and returned `1` on the server. Utility `SET` was not sent.

## a080 measurement

Linux arm64, Rust 1.94.0, image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, container `oo-a080-msrv-f669cc94`, `--cap-drop ALL`, no new privileges, bridge network, no home mount. The immutable baseline before these edits passed locked tests and Clippy for the default features and for `test-hooks`. `apt-get update` inside that capability set exited 100 and was left as a baseline failure. No second builder was started. The semantic corpus was not reblessed. No `DROP`, `set_config`, or `pg_read_file` was sent to PostgreSQL. The PG 17.10 fixture was not rerun.

On the corrected code, the default corpus worker path returns the existing codes with `worker_identity=pinned` and `containment=proven` when a process is spawned. `EMPTY_SQL` and `OVERSIZE` do not spawn. Adversarial workers under `test-hooks` cover the contradictory allow matrix, a `DROP TABLE` payload cleared to empty SQL, false OID, spoofed pid/starttime/nonce, malformed frames, separate 5 s and 500 ms clocks, held stdout, same-group and `setsid` children, foreign `sleep` survival, the file-size cap, and the explicit network/credential boundary. Outer wall time includes copying the worker image into a memfd (about 2.7 s on this builder). The readiness, parse, and cleanup fields are the lifecycle clocks. A fast bad frame returned `ready_wait_ms=1` and `cleanup_wait_ms=18`. A silent worker's readiness wait stayed inside 4.9–5.6 s.

## Consequences

SP06-W02 still owns R4 OID binding, authenticated policy, EXPLAIN budgets, RLS, and database enforcement. This crate must not become a Gateway dependency. AST shape does not prove function, operator, or cast OID safety, and it does not prove ACL or RLS.

These cases stay fail-closed or explicitly open, so this slice is not R1–R9:

- `NATURAL JOIN`, outer `JOIN USING`, a `JOIN USING` alias, `LATERAL`, whole-row references, recursive CTE binding, and nested `JSON_TABLE` columns are denied with a deferred or unresolved code.
- `WHERE true` on `analytics.big_fact` is `ALLOW_SYNTAX`. The deparsed `SELECT b.id, b.ts ... WHERE true` returned the row on the a065 fixture. Raw `SELECT *` was `42501` because `amount` is not granted. That is column ACL, not R9. R9 stays open.
- `SELECT *` from a subquery that already projects `b.id, b.ts` is `ALLOW_SYNTAX` and returned the row on the a065 fixture. The wrapper clears the large-table flag. That wrapper case stays open.
- The vendored parser rejects 16 nested scalar `SELECT`s before the walker can see depth 128. The depth-128 deny is a unit test on synthetic JSON. The corpus depth case expects `SQL_PARSE_ERROR`.
- `SELECT set_config('oo.claims', ...)` is `ALLOW_SYNTAX`. It was not executed. R4 is open.
- `agent_reader` could `SELECT relname FROM pg_catalog.pg_class` (431 names) on the a065 fixture. The parser returns `FORBIDDEN_SCHEMA`. Catalog database permissions (R5) stay open. The fixture was not rerun in a080.
- Hidden-column selects are `COLUMN_NOT_ALLOWED` in the spike and SQLSTATE `42501` with the text `permission denied for table` on the server.
- Unknown protobuf fields that never appear in the JSON walk are not detected if the reported parser version stays 170007.

## Switch condition

Revisit when a crates.io `pg_query` release that is at least seven days old embeds PostgreSQL 17.10, or when SP06-W02 takes the worker protocol into the Gateway. A major mismatch still refuses data readiness under REQ-GW-03. Acceptance of P5 requires the deferred list above to be closed or explicitly waived by the component and security owners.

## Requirements and acceptance

| Kind | Identifier | Scope |
|---|---|---|
| Requirement | `REQ-GW-03` | Parser subset: caps, deadline, killable worker, version fail-closed. Full Gateway readiness was not run. |
| Acceptance | R1–R3, R5–R8, R7a, part of R9 | Syntax and synthetic policy only. See the open list. |
| Acceptance | R4, EXPLAIN budgets, ACL, RLS | Not run. |
| Prototype | `P5` | Not accepted. This is a subset. |

## Compatibility and migration

- Protocol: magic `P05P`, version `1`, kinds ready/admit/response plus test-only fault kinds. SQL max 65536 bytes. Frame body max 256 KiB. AST max 10000 nodes, depth 128. Readiness 5 s. Parse deadline 500 ms after ready. Cleanup 500 ms. Workdir byte budget 256 KiB. Worker `RLIMIT_FSIZE` 256 KiB.
- Lock: this spike's own `Cargo.lock`. It does not edit the root lock.
- Old binary window: none. There is no deployed parser worker.
- Rollback: delete this spike. No migration.
- Source database write: none. The fixture creates synthetic tables inside an unpublished container and rolls nothing into a source database.

## Security and supply chain

- Trust boundary: native parser IPC and SQL admission. The worker image is the pinned memfd described above. The worker's environment is cleared, `PATH` is `/usr/bin:/bin`, and the workdir is an empty temp directory. Cleared environment and a temp cwd are not a network or credential boundary. Syntax output is not database authorization.
- New dependency: none in a080. `pg_query` 6.2.0 and its native tree stay as recorded in `DEPENDENCY-PROVENANCE.md`. License bodies are in `licenses/`. The protobuf-c excerpt is the compiled header's 2008–2018 copyright. The Bison 2.3 skeleton is GPL-2.0-or-later with the Bison special exception. That is not a blanket GPL exception. Upstream MIT, BSD-3, and PostgreSQL texts were fetched and matched. That match is not a full supply approval. Age floors and the root policy are unchanged.
- Secrets: none. The a065 fixture password string is not a production credential. a080 did not start a database.
- Data: synthetic SQL strings only. No customer data.

## Evidence

`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a065/SP01/P5/REPORT.md` is the original measurement. `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a080/SP01/P5-ipc-correction/REPORT.md` is the a080 correction. An empty reviewer table means this ADR stays Proposed. a080 does not accept P5, Gateway authorization, R4, R5, R9, or native 17.7 versus server 17.10 grammar parity. amd64 and gVisor were not run.

## Review

| Role | Person | Conclusion | Date |
|---|---|---|---|
| Component owner | unassigned | | |
| Security owner | unassigned | | |
| Supply-chain owner | unassigned | | |
