# Handoff

## Status

Reviewed PostgreSQL composition evidence is recorded for session `eef831aa-dba4-4af2-a78b-541bc9e0a226` at fixed commit `ed661809cf6c1e8df37af5842838dd1eb839c98a`. `live/outcome.txt` is `reviewed_pg_composition_recorded`. `live/outcome.json` has `parent_acceptance=false`, `development_unit=false`, and `component_approval=false`.

Do not count this directory as a development unit. Do not mark M1-I01 or M1-I02 accepted. Do not treat the run as component approval for `reviewed-effective-oos`, `install_column_acl_set`, `analyze_mapped_shadows`, `init-pg`, `role-pack-bundle`, or `query-pg`. Those compilers are already in the fixed archive. This run is the composition that a203 and a211 could not finish because the bridge between them was missing.

## Still missing

- Semantic publication. Import state is `awaiting_publish`. Publication count is 0.
- Authentication and authorization. Compiler statuses are `not_claimed`.
- Library behavior for login mapping privileges. After `install_column_acl_set` the login had no `USAGE` on `_oo`, no `SELECT` on `_oo.physical_mappings`, and no `EXECUTE` on `lookup_physical_mapping`. The fixture grant is labeled `fixture_configuration_not_library`.
- A column-ACL CLI. Role and login binding is configuration, labeled `configuration_not_auth`. Grantor was `oo_fixture_admin`.
- A WAL. Query audits are local JSONL files.
- Natural language. Harness mount was `not_performed`.
- Business-column grants were not substituted for the compiler. Policy labels were taken from the emitted effective document.

## Queue

Leave the pending component-review list as it stood: a215, a224, a217, a218, a219, a223. This run does not close any of them.

## Evidence map

- `REPORT.md`, `FINDINGS.md`, this file.
- `live/outcome.json` and `live/steps/` for raw stdout, stderr, and exits.
- `linux/smoke/` for the pre-overlay focused tests (`smoke.exit` 0).
- `linux/chain/` for the successful overlay, clippy, and live run (`chain.exit` 0, `docker-run.exit` 0).
- `linux/chain/attempt-1` through `attempt-4` for preserved failures. `attempt-standalone/why.txt` records the skipped standalone lock path.
- `origin/recipes.txt` for recipe commits and blob ids.
- `source/blobs/` for the compiler sources copied from the fixed archive.
- `SHA256SUMS` from the Linux `sha256` tool after cleanup.

## Builder limits that held

One a226 rust-builder at a time, 6 GiB, 1 CPU, `cap-drop ALL`, no privileged mode, no host network, no host home, no shared cargo cache. PostgreSQL was one private container on `oo-a226-eef831aa-net`. Host `cargo`/`rustc`/`rustdoc`/`rustup` processes were 0 at each launch. Global rust-builder count stayed under 10. Other owners' containers were not stopped.

## Cleanup and commits

`cleanup/run.sh` deletes only this task's containers, volumes, network, and `/tmp/oo-a226-eef831aa-dba4-4af2-a78b-541bc9e0a226`. Pinned images stay. The ledger and product tree stay untouched. Commits, if present, are only under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a226/SP11/reviewed-pg-composition/` and are not pushed.
