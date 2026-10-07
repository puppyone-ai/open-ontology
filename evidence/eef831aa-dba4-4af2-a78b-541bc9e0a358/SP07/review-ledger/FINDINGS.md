# FINDINGS

Bound to product commit `f862683945a6e5a8803e3f9450d0f51cde5a98ad`. A green test does not accept SP07-W05.

## Held by the code that was tested

The control file must say `authority=not_verified` and `management_identity=not_claimed`. Exactly one role is stored: `reviewer`, `owner`, or `auditor`. The credential is SHA-256 of a private file, not a request header. Scope is an exact object id. `public.b` does not authorize `public.b.id`. Grant, publish, apply, role, and `auto_accept` keys are refused before the decision is parsed. The live CLI check left the decision count unchanged.

A stored decision stays `curated_applied=false`, `published=false`, `authenticated=false`, and `grants=not_authorized`. `owner_certification` from the owner principal stores `record` and still does not grant. A reviewer attempt stores nothing and returns `reviewer_not_owner`.

Label and description approvals accept only the declared fields. Classification accepts only `pii` (`none`, `personal`, `sensitive`), `semantic_type`, and `state=confirmed`. Missing or `unknown` PII returns `pii.unknown` and writes no decision. The batch partial case kept the approved object and refused the unknown-PII object. `accept_all` writes no batch row.

The description-review receipt ran `apply_description_review` outside the decision transaction, then stored one receipt bound to the command hash, candidate hash, and outcome hash. The decision body was unchanged. The curated directory stayed empty. A later head candidate made another receipt `candidate.changed` and left the existing receipt count at 1.

The poison check inserted an audit row for `poison.1` as admin, then `decide` returned `audit.rollback`. The decision row for that key was absent. The one remaining audit row is the pre-inserted poison row. `finish` rolls the transaction back on error.

Runtime `SELECT … FOR UPDATE` on `decision` required an `UPDATE` privilege. The migration grants it. The decision trigger still rejects `UPDATE` and `DELETE`. The passing install ran after the database was dropped and recreated, so the stored manifest hashes that grant. The restart used that data directory.

## Remaining management, HTTP, and Git work

These are not implemented in this crate:

- Management OIDC and RBAC. The local-operator fixture is a file credential. It is not a verified management identity.
- An HTTP control API. The callable surface is the Rust library and the `review-ledger` CLI.
- Git apply of curated files, and any write of curated documents. `git_applied` and `curated_files_written` are false.
- Publication.
- An audit collector and its certificate.
- Customer `GRANT`s and column visibility grants. A classification confirm does not make a column visible to an agent role.
- Production role bootstrap. The migration does not create `oo_review_ledger_admin` or `oo_review_ledger_runtime`. The proof created those roles, database `oo_a358_review`, and login `oo_a358_runtime` only inside this private instance.

## Limits of this proof

`commit.unknown` has no fault-injection test.

`install.drift` and `control.drift` are implemented and were not given a second SQL file or a second control file in the live matrix. The live install covered same-hash replay only.

A description-pack receipt that stores `packed` or `reviewed_unchanged` was not run. The live pack path stopped at `compiler.refused` for command `{`. The direct `apply_description_pack("{")` error code started with `oo.builder.description-pack-command/v1`. The ledger maps that to static `compiler.refused`.

`batch` does not refuse `role=auditor` before it records the batch. Item decisions from an auditor return `role.forbidden`, and `record_batch` can still insert the batch and its audit. The live matrix did not call batch as the auditor. Enqueue, decide, and receipt do refuse the auditor.

A receipt checks the caller's role and object scope. It does not require the caller principal to equal the decision's reviewer. The live receipt used the same reviewer fixture that stored the decision.

History was read as that reviewer. The function does not reject an auditor. An auditor history read was not in the live matrix.

The root `rust-toolchain.toml` channel is `1.98.1`. The first networked builder followed it and downloaded `1.98.1 (48a229cea 2026-09-01)`. Later commands set `RUSTUP_TOOLCHAIN=1.94.0` and checked `rustc 1.94.0`. The root toolchain file was not edited.

Clippy is not installed in the pinned Rust image. The clippy command added the `1.94.0` component inside that container. The component download is not a Docker image pull.

`Cargo.lock` was generated once inside this package with Rust 1.94 and then used with `--locked`. The root manifest and root lock were not changed. `cargo update` was not run.
