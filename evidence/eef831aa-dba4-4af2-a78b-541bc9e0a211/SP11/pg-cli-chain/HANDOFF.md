# Handoff

Session `eef831aa-dba4-4af2-a78b-541bc9e0a211`. Fixed commit `a661dcdf72ff91561ae6b7cb79916e582884d08a`. Owned path only: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a211/SP11/pg-cli-chain/`.

This handoff is the CLI-chain evidence record. It does not accept M1-I02. It does not check a parent box. It does not release. Approval stays pending a208. a205 REQUEST_CHANGES stays in force. a203 remains the API integration on the older base and is a different claim. a207 remains the only connector-manifest and root-lock writer.

## Read first

- `REPORT.md` — what the final run executed.
- `FINDINGS.md` — the seams, with file names.
- `live/outcome.json` — exits, hashes, generations, and the ACL refusal text.
- `live/steps/` — raw stdout, stderr, and exit of each compiled command.
- `linux/chain/phases/` — smoke-equivalent focused tests, bins, overlay, lock diff, clippy, run, secret scan. Every final phase exit is 0.
- `linux/chain/attempt-1` through `attempt-9` — failed attempts, each with `why.txt`. Do not delete them.

Final container `167ec9fb393ce994e26bf81793dd8aba5c0ec2124470e84b684914eed55f9892` exited 0 and was removed. `OOMKilled=false`. Builder limit was 6 GiB and 1 CPU. Postgres image and Rust image were the pinned digests already present locally. `--pull never`.

## Seams the next reader must keep

1. Role pack. Review stdout is not a role-pack command. Wrapping the structure draft as `effective_oos` is rejected with `schema.rejected`. There is no CLI that produces effective-oos. Do not invent a pack.

2. Column ACL. `install_column_acl` grants the first table of a role. The second table returns `existing column grants do not match` and adds nothing. Ledger was not granted. Clerk and peek `query-pg` successes are party only. The ledger request exited 6, guard, with no audit file. Do not hand-GRANT the second table.

3. Publication. Import state is `awaiting_publish`. `target_generation` is 1. Mapping `run_generation` is source generation 11. `pg_publication` count is 0. Draft publication is `unavailable`. Effective `pii` and `semantic_type` are null. Semantic publication is missing. Authentication is not claimed. No harness was mounted.

4. Catalog stats. `import-pg` leaves `reltuples` at `-1`. `query-pg` with `unknown_rows=refuse` exits 5 until admin `ANALYZE` of the imported physical tables. The before-analyze exit is preserved. The ANALYZE is labeled and is not an import CLI step.

5. Installer. `import-pg` does not install schema. The not-ready command exits 1 with `target schema is not ready`. Column ACL has no CLI. Both calls are the public library APIs, named as such in `outcome.json`.

## Rebuild notes

Archive extraction used for the builds lives under the private tmp directory recorded by the launch scripts, not in git. Product sources copied at smoke time are under `source/blobs/`. The overlay path is required: a standalone lock that drops registry packages is not the frozen set. `--lockfile-path` is unstable on cargo 1.94 stable and must not be used. Registry triples of the overlay lock matched the archive (`lock-registry-added.txt` and `lock-registry-removed.txt` empty).

The harness orders the direct peek `SELECT` by the granted `party_id` physical column. `_oo_pos` is not part of the column grant, and ordering by it returns permission denied for the physical table.

## Cleanup expectation

Own resources only: containers `oo-a211-chain` (already removed) and `oo-a211-pg`, volumes `oo-a211-eef831aa-cargo`, `oo-a211-eef831aa-target`, `oo-a211-eef831aa-pgdata`, network `oo-a211-eef831aa-net`, and `/tmp/oo-a211-eef831aa-dba4-4af2-a78b-541bc9e0a211`. No prune. No foreign container. Pinned images stay. `CLEANUP.txt` records the commands that actually ran.
