# Handoff

Source is `4ed22b8c1189e72b06b20a45335860ac9a3ea44e` on `master`. The evidence commit that cites this file is the next commit. Do not reopen `0000` through `0004`. Do not treat `target_installer` as the Loader or as source-reader admission.

## What is public

`install(&mut PgConnection)` and `verify_readiness(&mut PgConnection)`. Digests come from `compiled_digests()`. The migration lock key is `6404202605`. Callers pass an idle connection. Bootstrap needs a superuser. Upgrade and replay need a superuser or a member of `oo_target_owner`. Runtime readiness is a read-only transaction and cannot install.

## What the green probe already proved

One private PostgreSQL 17.10, six fixture databases, sequential. Empty install, immutable replay, direct v1 without a v2 marker, v2 history and one legal receipt retained, `OO002` with no marker, `OO001` with the lie unchanged, `OO021` with no readiness, runtime verify and runtime refusal. Details and the raw server version string are in `REPORT.md` and `linux/probe/live/outcome.txt`.

## What is still open

A database that already has a valid `target-fence-receipts/3` row and no `physical-mapping/v1` row is specified to run `0004` only. That shape was not created in the live fixture. The next installer test can add that one database if a later review wants the route exercised on a server. It should stay on one PostgreSQL and should not grow into a privilege matrix.

`schema_ready`, `segment_contract_ready`, and `segment_preflight_ready` still take a caller digest. Production code that needs a verified target uses `verify_readiness` and the returned token.

## Containers

a150 containers, the a150 network, and the a150 cargo, target, and postgres volumes were removed after the green probe. The private `/tmp` directory that held the smoke archive and the database password file was removed. The password was not in the evidence tree. `docker prune` was not run. No foreign container was removed by name.
