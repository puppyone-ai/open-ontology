# FINDINGS

Bound to amendment product commit `6b3a92c2e06a2e91a74393b739ec8f1c565ae17b`. A green test does not accept E52 or SP07-W05. The prior packet for `f862683945a6e5a8803e3f9450d0f51cde5a98ad` is still in git history. Two gaps named there are closed here. The other limits stay open.

## Closed on this commit

An auditor `batch` returns `role.forbidden` before any batch row, batch item, decision, or audit insert. The live call used in-scope `public.b` and `accept_all: false`. Counts of `batch`, `batch_item`, `decision`, and `audit_event` were unchanged. Key `auditor.batch` is absent. `bauditor` is absent.

Auditor `history` is allowed. It returned the same project digest as the reviewer and inserted no audit row. History does not filter rows down to the auditor scope `public.b`.

The live description-pack receipt for `class.b.note` is `bound`, then `replay`, with one row. The compiler outcome of that command file is `packed`. The receipt stores the command hash, the decision candidate hash, and the compiler-document hash named in `REPORT.md`. `git_applied`, `published`, and `curated_files_written` are false. Decision `class.b.note` stays `curated_applied=false`, `published=false`, `authenticated=false`, and `grants=not_authorized`. The malformed command `{` is still `compiler.refused` and writes no receipt.

`commit.unknown` is an ambiguous lost reply. The contract test checks that `Output::from_error` uses kind and code `commit.unknown`, leaves `authenticated` and `curated_applied` and `published` false, sets `grants` to `not_authorized`, and omits `history_sha256`. There is no fault-injection test and no blind retry.

## Still held from the tested crate

The control file must say `authority=not_verified` and `management_identity=not_claimed`. Exactly one role is stored: `reviewer`, `owner`, or `auditor`. The credential is SHA-256 of a private file. Scope is an exact object id. `public.b` does not authorize `public.b.id`. Grant, publish, apply, role, and `auto_accept` keys are refused before the decision is parsed.

A stored decision stays unapplied and unpublished. Label and description approvals accept only the declared fields. Classification accepts only `pii` (`none`, `personal`, `sensitive`), `semantic_type`, and `state=confirmed`. Missing or `unknown` PII returns `pii.unknown` and writes no decision. `accept_all` writes no batch row. The batch partial case keeps the approved object and refuses the unknown-PII object.

The description-review receipt still runs outside the decision transaction. The curated directory stayed empty. A later head candidate made another receipt `candidate.changed` and left the existing review receipt count at 1.

The poison check still returns `audit.rollback`. The decision row for `poison.1` was absent. The one remaining audit row for that key is the pre-inserted poison row.

Runtime `SELECT … FOR UPDATE` on `decision` still uses the `UPDATE` grant from the unchanged migration. The decision trigger still rejects `UPDATE` and `DELETE`.

## Remaining management, HTTP, and Git work

These are not implemented in this crate:

- Management OIDC and RBAC. The local-operator fixture is a file credential. It is not a verified management identity.
- An HTTP control API. The callable surface is the Rust library and the `review-ledger` CLI.
- Git apply of curated files, and any write of curated documents. `git_applied` and `curated_files_written` are false.
- Publication.
- An audit collector and its certificate.
- Customer `GRANT`s and column visibility grants. A classification confirm does not make a column visible to an agent role.
- Production role bootstrap. The migration does not create `oo_review_ledger_admin` or `oo_review_ledger_runtime`. The proof created those roles, database `oo_a358_review`, and login `oo_a358_runtime` only inside this private instance.

## Limits that remain

`install.drift` and `control.drift` are implemented and were not given a second SQL file or a second control file in the live matrix. The live install covered same-hash replay only.

A receipt checks the caller's role and object scope. It does not require the caller principal to equal the decision's reviewer. The live pack receipt used the same reviewer fixture that stored the classification.

The root `rust-toolchain.toml` channel is `1.98.1`. Commands set `RUSTUP_TOOLCHAIN=1.94.0` and checked `rustc 1.94.0`. The root toolchain file was not edited.

Clippy is not installed in the pinned Rust image. The clippy command added the `1.94.0` component inside that container. The component download is not a Docker image pull.

`Cargo.lock` was already present. This round used `--locked` and did not generate or update it.
