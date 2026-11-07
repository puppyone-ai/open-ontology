## Card B-S3d — lane B: close the L2 re-review follow-ups

TODO: S1-B-01, S3-B-01, S3-M-01. Re-review at d84d2aa43: findings 1,2,3,5,6 fixed; #4 partial; three new minor defects.
Fix each with a test that fails before and passes after; one commit each. Work rule: keep going; commit as you go.
1. Rate limits (crates/oo-gateway/src/auth_lifecycle.rs:66-86): the global cap (80/min) is below the sum of route
   caps (90), so routes can starve each other; renew is globally capped at 10/min although it is authenticated.
   Make the global cap ≥ the sum of route caps, and limit self-renew per token (authenticated) instead of globally.
   Document behavior behind a load balancer (all clients share the balancer's peer bucket) in CONTRACT.md; if a
   trusted forwarded-for setting already exists in the Gateway config, honor it only from configured proxies.
2. Device polling (auth_lifecycle.rs:74, auth_device.rs:202): cap per device code and per peer rather than 60/min
   across all clients, so concurrent device logins are not throttled together; unknown device codes are rejected
   before opening an admin connection (keep auditing them through the cheap path or a sampled counter — state which).
3. crates/oo-cli/src/lifecycle.rs:880-886: when issue/enroll/rotate cannot read or validate the secret, delete the
   reserved empty output file and print the token hash so the admin can revoke it; non-zero exit.
4. `oo token` must call negotiate() (minimum CLI version, exit 9) like login/whoami/share-prompt.
Then, if `df -g /` shows ≥ 22 GB free, run the owned-PG runners (crates/oo-gateway/tests/token_owned_pg.py and
packages/cli/test/native-owned-pg.py) to verify findings 1 and 6 against a real database; otherwise record not run.
