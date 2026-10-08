# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a394`. Independent review of a387 `tools/session-control-api` at commit `53017ed29c98c49fb44a7eb18d5df6d5de273046`. Read `REPORT.md` and `FINDINGS.md` first. This handoff does not accept the card, SP11, or the parent goal. The ledger was not edited.

## Observed

Localprocess HTTP/1.1 on loopback, grant plus bindings file, PostgreSQL session, turn, and event, then cancel. `oo_token_control.revocation` went from 0 rows to 1 while `token_record` stayed at 1. `proxy_mapping` went `present` to `deleted`. `permit` went `held` to `released`. Those row changes were sampled while a release-spawned `/bin/bash` child, or the `pg_sleep` backend, was still alive. S5 was skipped on an absent snapshot. A present snapshot returned 409 and left the phase unclosed. S8 absence was the workspace directory gone after close. A killed server process was replaced and the new process returned 200 `closed`. Replay cancel stayed at one revocation row. Cross-person, cross-project, stale revision, and old generation were refused, and the head stayed at revision 3 and generation 1.

`reducer_physical_release_certified` was false on the HTTP receipt and on `session_store::get`. `production_signed` was false. Production `serve` exited 2 with `runtime_unavailable` and wrote no port. `fixture_persisted` was phase `releasing` and was not counted as a close.

## What this review does not do

It does not sign production. It does not set a certification flag. It does not accept SP11 or the parent goal. It does not treat the release-spawned bash fixture as a pre-owned child. It does not treat the constant `map_ip` `10.88.0.8` as an external proxy. It does not shrink the original card. The HTTPS verified-identity sentence in the plan is not implemented by this frozen API.

## Re-run

Parent at this writing: `d1124b0a169bca198414ba81dec7623e1550c270`.

Images, unchanged:

- `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`

Recipes: a387 `linux/impl/launch.sh` sha256 `c522fe7bd898d640bbf9d9b4047d2bec74bb18c81e3c04fd7c75d1448bf8b88e`, `inside.sh` sha256 `c88f4b4d71fa919b81555a8d54678640cb2a92c5496770fb2b7ed8ce38e0abe3`, `pg/up.sh` sha256 `ce993ed95722bb098e8a5db42a10ee42426670e843b86809b5e8be446099d72b`, `pg/pg-wrap.sh` sha256 `25712207f986b11e122bd992d5a695881e1e86625e0edf2d0b683b023ebfdd2f`. The a394 wrapper matches that wrap hash. Names, database, and network are a394-private.

Keep the archived lock. Do not run `cargo update` or `generate-lockfile`. Do not edit the root manifest. One Rust builder, 6 GiB, 1 CPU. One private PostgreSQL 17.10, 1 GiB, 1 CPU.

## Boundaries

Owned path: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a394/SP11/session-control-api-review/**`. Product crates were read from the archive and were not edited. No customer call, push, or release was made. Owned cleanup is `cleanup.txt`.
