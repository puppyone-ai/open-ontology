# Handoff

Independent review is next. This session did not review its own commit as that review, and it did not mark `specs/open-ontology-development-ledger.md` or `specs/open-ontology-acceptance-counting.md`. M1-D14A stays `未验收a218`.

## Commit

Product `543580768dec60d127eb428c09e0a9a400b18272` on parent `5394c70442f34dc54e023d2193d9a336193daa8a`.

The commit is only:

- `crates/oo-gateway/src/policy.rs` blob `84a86e85ef82bc2a459a5e2faef5827d077ca6cc`, content SHA-256 `68d6a1639263f6e7083eb44244360dfd22f73897fb3eff2570052da6b508188d`
- `crates/oo-gateway/tests/column_acl_set_pg_live.rs` blob `69897c46a1f3c5607936b0dffc3a63d3824ce2e4`, content SHA-256 `68f96b3ce11c01f612329d150f64c88e1b087982bdd9a5a6e772156fad24d53b`
- `contracts/gateway/column-acl-set-v1.md` blob `1bc75704e7930fea79b743c55a9f0721697fa3e2`, content SHA-256 `476cfe39d70b53bd9cf1f62f5047c757e36aa276ff13ee0b62b1600ac17b3366`

Fixed archive for both Linux runs: `c2db9c430b822a824a1062b6c3c7345924df7dd8`. Parent `policy.rs` matches that archive byte for byte.

The evidence commit adds this directory. Its hash is the commit object. These files do not contain it.

## Reproduce the green check

One Linux arm64 Rust 1.94 container, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, private cargo and target volumes, `--pull never`, `--memory 6g`, `--cpus 1`. One private PostgreSQL 17.10 container, image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, TLS, no host port. Archive `c2db9c430b822a824a1062b6c3c7345924df7dd8` and copy in the three owned files. Set `OO_A218_LIVE=1`. Commands:

```text
cargo test --locked --offline -p oo-gateway --lib policy::tests -- --test-threads=1
cargo test --locked --offline -p oo-gateway --test column_acl_set_pg_live -- --test-threads=1 --nocapture
cargo clippy --locked --offline -p oo-gateway --lib --test column_acl_set_pg_live -- -D warnings
```

The green transcript is `linux/proof/console.log`. `proof.exit` is 0. The pre-edit smoke is `linux/smoke`. Failed proof attempts 1, 2, and 3 stay under `linux/proof/`.

## Review focus

- `install_column_acl` still returns `GrantMismatch` for a second table on an existing role. The aggregate function is the multi-table path.
- Compile, generation, mapping, type, PII, and row-filter checks finish before `CREATE ROLE`. The live bad-second cases are `MappingAbsent` and `TypeRefused`, and both roles are absent. A post-grant read failure would roll back a role created in that transaction. The live fixture did not inject that fault between the two grants.
- Replay requires the full mapped column set, no table-wide privilege, hidden `_oo_pos`, and membership of exactly the policy login with inherit false, set true, admin false. Partial, changed, and foreign sets return `GrantMismatch` and leave grants in place. A second login returns `MembershipMismatch`.
- Digest B at generation 9 does not raise digest A's current generation. Generation 9 on digest A is `StaleGeneration` while digest A's maximum stays 7.
- `query-pg` clerk and peek exit 0 for both tables, count, and star. Peek stdout omits `secret-note` and `memo-a`. Guard exit 6 writes no audit file. Direct hidden reads, writes, and raw `SELECT *` are `42501`.
- The login's mapping `USAGE` / `SELECT` / `EXECUTE` is a fixture grant so catalog load can run before `SET ROLE`. It is not an installer grant.
- The grantor in the proof is `oo_fixture_admin`. The installer does not `ANALYZE`. The test does, after the grant assertions. Publication stayed 0. Authentication, RLS, and the audit WAL were not claimed.
- `lib.rs` was not edited. The new function is reached through `oo_gateway::policy`.

## Still open

Authentication, row-level security, claims binding, publication, the audit WAL, a non-superuser grantor, a connection pool, and the parser worker. Whole SP06-W01 and M1-D14A remain open. No component was added. No manifest or lock was edited.

`oo-a218-smoke` and `oo-a218-proof` were removed by their launch scripts. After evidence commit `4df5d9165193adf5307a0bdf9f690ca3679b577f`, `cleanup/run.sh` removed `oo-a218-pg`, volumes `oo-a218-eef831aa-cargo`, `oo-a218-eef831aa-target`, and `oo-a218-eef831aa-pgdata`, network `oo-a218-eef831aa-net`, and `/tmp/oo-a218-eef831aa-dba4-4af2-a78b-541bc9e0a218`. The script checks owner `a218` and this session. It does not prune. The transcript is `CLEANUP.txt`. Pinned images stayed.
