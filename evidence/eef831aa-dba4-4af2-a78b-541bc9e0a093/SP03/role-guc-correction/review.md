# Fixed review a093 role GUC identity

Reviewer: same session, `eef831aa-dba4-4af2-a78b-541bc9e0a093`. No separate reviewer was started. Subject: `34fae5ddd73ff250fde6f51aa681fb0f7fbd0362`.

## Verdict

The identity correction matches the checks that were run. Registration stays blocked. The catalog stays refused. The TLS requirement stays verify-full with the caller CA. I would not call this a full source acceptance.

## Checked

- RED on the frozen baseline panicked with `Tls` / `tls_not_negotiated` at the GUC connect, after the earlier fixture assertions in that test had already run. The production edit followed that panic.
- The accepting fixture run, same server, final overlays: reader registration with `server_observation=observed` and a `TLSv1.` version; GUC login `session_user=oo_src_guc_login`, `current_user=oo_src_side`, `role_mismatch` / `session_user`, `startup_option` / `startup_options`, `negotiated_version=unknown`, `server_observation=unknown`, write state `unknown`, empty memberships and definers, `catalog` `Config` / `session_user_mismatch`.
- The same test still rejects wrong CA and wrong host, keeps the admin / NOINHERIT / SET-only / session-mismatch / write-role controls, and sets hostile `PGOPTIONS` only on the child process.
- `IDENTITY` is one `SELECT`. The lib test rejects `SET ROLE`, `RESET ROLE`, and `SET SESSION AUTHORIZATION` in that statement. The accepting log has only `SELECT`, `WITH`, `START`, and `ROLLBACK`.
- `ssl=false` is still `tls_not_negotiated`. A version string without `ssl=true` is `tls_version_unknown`. A missing row with a missing version is `unknown`.
- Clippy `-D warnings` exit 0. Lib 19 passed, which is the baseline 14 plus the five new unit tests. Connector manifest and root lock are absent from the commit. Index check before the commit showed those paths unstaged.
- Password-file hits and the comment canary hits on both the RED log and the accepting log were 0. Counts only; the secrets were not printed.

## Notes that are not defects

`catalog` checks identity before startup options, so the GUC catalog fact is `session_user_mismatch` while `inspect_access` carries both block reasons. That is the required catalog fact.

The startup filter now also includes `user`, `database`, and `database user` sources outside the existing allow-list. The fixture reader has no such row, and the reader still registers. A role whose only extra parameter is an allow-listed name such as `TimeZone` still passes the filter. A non-allow-listed user-level parameter would now block. This fixture does not include that extra case.

`server_observation` is derived from the stored version string. The only non-`TLSv1.` value `read_identity` stores is the literal `unknown`.

The first green container failed to compile lib tests because `unwrap` needs `Debug` on `ServerTls`. The accepting container includes `#[derive(Debug)]` on that private enum. Behavior of the classifier did not change between those two live passes; both live exits were 0, and the second is the recorded acceptance.

The Rust containers used `--cap-drop ALL`. The PostgreSQL fixture did not, because that image's entrypoint uses its normal privileges. It was not `--privileged`, not host networking, and it published no ports.

Validation used the baseline archive plus four file overlays. It did not compile the shared worktree. HEAD's lock adds two builder dependencies and does not change the connector package section.

## Not accepted here

PostgreSQL 13 and 14, lossless types, bounded reader, import, CLI, supply-chain review, and amd64 were not run. a087's missing-log-mount 101, oracle `set_var` compile 101, oracle runtime 101, and cargo-deny history remain in that evidence and were not rewritten. The baseline package test without `OO_PG_FIXTURE` remains exit 101.

No release, no customer access, and no claim that SP03 is closed.
