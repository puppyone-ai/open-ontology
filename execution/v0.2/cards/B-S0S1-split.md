## Card B-S0S1-split — lane B: split the token lifecycle commit per the commit standard

TODO: S0-B-01, S1-B-01, S1-B-02. Lane label: `b`.
Your previous run delivered the functionality in one commit `6cc92925c` (2,708 lines, 11 files) whose body
contains literal `\n` sequences. Functionally it passed sign-off; the history does not meet plan §10.3.

Rewrite your lane branch (it has not been pushed or integrated, so rewriting it is allowed) into logical
commits with the same final tree, e.g.:
1. `feat(token-store): add forward-only lifecycle migration` (oo_token_control_v2.sql + its tests)
2. `feat(token-store): add lifecycle operations and audit` (src/lifecycle.rs + unit tests, CONTRACT.md part)
3. `feat(gateway): add admin lifecycle API and issue-token subcommands` (auth_lifecycle.rs, issue-token.rs, http_gateway admin parts)
4. `feat(gateway): add public enroll and self-renew endpoints`
5. `feat(gateway): add /v1/meta and expiry health`
6. `test(gateway): add owned-PG lifecycle acceptance runner` (token_owned_pg.py, token_meta_pg, EVIDENCE.md)
Adjust boundaries so every commit compiles (`cargo check --workspace --locked`) and its tests pass. Bodies with
real line breaks (use `git commit -F`). Verify `git diff 6cc92925c HEAD` is empty at the end. Then re-run the
exit command once and report.
