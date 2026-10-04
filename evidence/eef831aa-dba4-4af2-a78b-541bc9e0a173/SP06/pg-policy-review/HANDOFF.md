# Handoff — a173

## Result

**APPROVE LIMITED** for `d3429726618a4e69ada7fb35131f9043a21e23e9`. The public column-ACL primitive installs one `NOLOGIN` role with column `SELECT` on the mapped shadow, on a trusted TLS session, with a superuser grantor that is not `oo_target_owner`. The deciding oracle is `linux/probe-4/live/oracle.txt`. Source blobs and blob ids are in `source/` (commit `997eb638f546cd7c73fa77a56e07591678f17780`).

Read `FINDINGS.md` before treating the TLS error text, the architecture list, or the license strings as closed. `db_error` drops the rustls source. `tokio-postgres-rustls` is outside the four-name gateway stack list. `cargo deny` and `cargo audit` were not run.

## Still open

Administrator and login authentication, the audit WAL, the parser, portal execution, `CancelToken`, the pool, RLS, claims binding, publication, and a non-superuser grantor. `SET LOCAL oo.claims` still succeeds from a role that holds `SET`, and the hidden column stays `42501`. That is not an RLS proof. A functional probe is not a release approval and not an SBOM.

## Do next

No product change is required for this limited approval. A later change that wants the rustls reason in `PolicyError` can keep the source chain in `db_error` (`policy.rs` lines 546–557) without weakening `SslMode::Require`. Closing the adapter gap means an explicit taxonomy decision, not a silent edit that adds `tokio-postgres-rustls` to `is_gateway_postgres_stack`.

Leave frozen migrations, the connector, pipeline, core, builder, the architecture gate, control, spec, and the ledger unchanged.

## Review

Grok 4.7 in this session was the only coder, tester, and reviewer. No subagent and no worktree. The author session that produced `d3429726` is a different session. No parent checkbox was changed. No release is claimed.

## State left behind

`cleanup.txt`. Own containers, volumes, network, and the `/tmp` secret directory are removed. Each removal exited 0. Failed probe attempts 1 through 3 and the failed `pg/up.sh` attempt remain in this directory. Other tasks' containers were not removed. `docker prune` was not used.
