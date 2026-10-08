# Findings

## F1. `ls-tree` without `-r` rejected a valid binding commit

The first closed loop returned `403 git.rejected` from approve. PostgreSQL stayed at revision 1 and wrote no `binding_applied` row. `git ls-tree` on the tree lists `040000 tree … management`. The blob is `management/bindings.json`, one level down. `ls-tree -r` lists `100644 blob … management/bindings.json`. The author line already matched `oo-identity-binding <identity-binding@open-ontology.invalid> 1791417600 +0000`. The verifier now uses `ls-tree -r`. `git::tests::nested_binding_file_matches_the_commit` and the live approve both pass. Shape capture: `git-shape.txt`.

## F2. A caller who is the bootstrap subject hits elevation before frozen

`plan` rejects `subject == caller` with `binding.elevation`, then rejects a change aimed at the bootstrap subject and project with `binding.frozen`. Revoke of `fixture-admin` by `fixture-admin` is elevation. Frozen is reached when `person-deputy`, an admin on the same project, revokes the bootstrap row. That attempt returns `403 binding.frozen` and leaves the Git ref in place. `a402-proof.json` has both `self_revoke` and `bootstrap_frozen`.

## F3. `authorize` still reads the operator file

`GET /control/v1/identity/current` and login stamps use `binding_head` and `management_binding`. `authorize(session, config, …)` still compares the session with the file snapshot loaded at process start. The a370 suite calls it before any apply, and that suite passed. After an apply, the file and PostgreSQL diverge. A later in-process `authorize` call sees the file.

## F4. Install of an existing v1-only database is drift

A fresh install applies `oo_identity_control_v1.sql` and `oo_identity_control_v2.sql` in one transaction, seeds the file bindings with `git_commit` empty, and reports `disposition=committed`. The install report `sql_sha256` remains the v1 file hash. A second install with both hashes present reports `replayed` and does not reseed. v1 present without a matching v2 manifest is drift. Install does not upgrade that database.

## F5. Same idempotency key

The request hash covers effect, project, subject, role, expected revision, expected config commit, expected Git head, and the key. It omits the resulting Git commit. The same key and same hash return the stored receipt with `disposition=replayed` and do not insert a second audit row. The same key and a different hash return `409 idempotency.conflict` and do not change the binding. An identical grant records `unchanged`, requires the current Git head, and does not bump the revision.

## F6. Two writers

The consumer moves `refs/heads/management` with `update-ref --no-deref` only when `rev-parse` still equals the expected head. The provider then locks `binding_head` and updates it only when revision and config commit still match. The audit insert is in that transaction. The live pair `race-a` / `race-b` got one HTTP 200, one new audit row, revision plus one, and one of the two subjects. The Git command allowlist has no push, fetch, or prune. `binding_git` is absent from the config-commit preimage.

## F7. Harness notes that are not product defects

The first clippy run exited 101 because the Rust image has no clippy component until `rustup component add clippy --toolchain 1.94.0`. The component install is inside the ephemeral container. The first `psql` without `-U` failed because `POSTGRES_USER` is `oo_fixture_admin` and there is no `postgres` role. The a370 suite expects an empty `oo_identity_control` schema; it drops that schema only after the first install assertion. A later a370 run against the schema left by a failed binding loop reported `replayed`. Dropping the schema before the final a370 run produced `committed`.
