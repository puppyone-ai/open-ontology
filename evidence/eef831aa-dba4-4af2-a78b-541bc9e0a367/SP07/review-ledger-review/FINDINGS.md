# FINDINGS

Bound to product commit `6b3a92c2e06a2e91a74393b739ec8f1c565ae17b`. Verdict in `REPORT.md`: **APPROVE_LIMITED** for original E52-D1/1. SP07-W05 remains the larger parent: management RBAC, a review API and console, batch preview, complete PII and semantic type, and owner binding of certified evidence. Those parent clauses stay open. Plan line 491 was read and was not edited.

## Held on this commit

`batch` in `tools/review-ledger/src/store.rs` reads the control file through `open_caller`, returns `role.forbidden` for role `auditor`, and only then calls `connect`. `accept_all` still returns `batch.accept_all` before that read. The independent oracle measured this on database `oo_a367_oracle` with `track_counts=on`. Row counts and `pg_stat_user_tables` tuple counters for `oo_review_ledger` were unchanged. Key `auditor.batch` count is 0. Batch id `baudit` count is 0.

Auditor `history` returned kind `history` and inserted no audit row. Tuple counters were unchanged across that read.

Refusals that added no decision row: `reviewer.mismatch`, `scope.unauthorized` for `public.outsider`, `revision.conflict` at expected revision 9, `stale` after the input hash changed, and `reviewer_not_owner` for `owner_certification` from the reviewer principal.

Label approval `label.shop` inserted one decision and one audit. Replay inserted neither. The same key with choice `reject` returned `idempotency.conflict` and left both counts unchanged. Stored flags for that decision are `curated_applied=false`, `published=false`, `authenticated=false`, `grants=not_authorized`.

Audit key `poison.a367` was inserted by the superuser before the decision. The decide call returned `audit.rollback`. The decision count was unchanged and that decision key is absent. The decision insert and the audit insert share one transaction. `map_db` turns unique violation `audit_event_idempotency` into `audit.rollback`.

The positive pack path called the archived compiler. Outcome `packed`. Statements `not_claimed`, `unavailable`, `not_authorized`. A second call returned the same document bytes. Receipt `class.pack` stored command `36e1e461b388e7a6eaf084109f2709b7d291865db171fb8c6a02d60ebb6b00cb`, candidate `6161616161616161616161616161616161616161616161616161616161616161`, outcome `2317e15eac50d9aad9ae7f958212a7390e21da21f4749e6c015976946c0fb5d3`, protocol `oo.builder.description-pack/v1`. `git_applied`, `published`, and `curated_files_written` are false. Decision `body_sha256` was unchanged. Replay left one receipt row.

Command `{` is a compiler error whose code starts with `oo.builder.description-pack-command/v1`. The ledger receipt is `compiler.refused`. `compiler_receipt` count was unchanged. Decision `class.bad` stays unpublished, unapplied, unauthenticated, and `grants=not_authorized`.

`parse_control` refuses `authority=verified`, `management_identity=claimed` (`authority.not_verified`), and role `admin` (`control.role`) with no database call. `screen_request` refuses `grant`, `role`, `publish`, `authenticated`, and `auto_accept` as `not_grant`.

The author’s `private_postgres_matrix`, eight contract tests, and `sha256::tests::known_answers` passed on this cluster against database `oo_a358_review`. Product Clippy `--all-targets -D warnings` passed on Rust 1.94.0. Restart re-read history marker `c7e08e02cde66ab86b75394488f0d263ec9475cecc75884599de3703db8264ce`, byte-for-byte the author `proof-fix/history.sha256`.

`commit.unknown` remains the contract test `commit_unknown_is_ambiguous_and_not_a_success`. That test passed again here. It checks `Output::from_error`. This review did not inject a lost commit reply. The library still does not retry.

## Limits that stay open

The receipt runs in a later transaction. The contract calls that a second step. This crate never sets `curated_applied` or `git_applied` on the decision. A receipt is required before any apply, and this crate does not apply.

The receipt checks the caller role and the object scope. It does not require the caller principal to equal the decision reviewer. This oracle used the same reviewer that stored `class.pack`. A cross-principal receipt was not exercised.

The pack column in the oracle is `public.shop.pack`. Binding is the command-file SHA-256 equal to the decision `source_sha256`, plus the compiler document hash. The captured catalog inside `pack_fixture` is the archive fixture `public.b`. The ledger does not look up that column name inside the pack document.

`install.drift` and `control.drift` were not given a second SQL file or a second control file in this oracle. The author live install covered same-hash replay.

The migration still grants `UPDATE` to the runtime role so `SELECT … FOR UPDATE` can lock the head. Triggers reject `UPDATE` and `DELETE`. This oracle did not issue a direct `UPDATE`. The author finding stands.

History returns the project record. It does not drop rows outside the auditor scope. The contract says so. An auditor history call is still not a grant and not a publication.

The root `rust-toolchain.toml` channel is `1.98.1`. Proofs set `RUSTUP_TOOLCHAIN=1.94.0` and checked `rustc 1.94.0`. The toolchain file was not copied into the build tree. It was not edited.

Clippy is not in the pinned image. Both Clippy commands downloaded the `1.94.0` component inside the container. That download is not a Docker image pull.

Bootstrap in this cluster created `oo_review_ledger_admin`, `oo_review_ledger_runtime`, `oo_a358_runtime`, and `oo_a367_runtime`. The migration does not create those roles. This is not production role bootstrap.

## Outside this verdict

These surfaces are still absent from the crate, and this review does not accept them:

- Public management OIDC and RBAC. The local operator is a file credential. `authority` stays `not_verified`. `management_identity` stays `not_claimed`.
- An authenticated HTTP control API. The callable surface is the library and the `review-ledger` CLI.
- Git apply of curated files, and any write of curated documents.
- Publication.
- Full S9 and S10 review workflow, batch preview, and the parent owner-evidence certification.
- An audit collector and its certificate.
- Customer `GRANT`s and column visibility. A classification confirm does not show a column to an agent role.
- The later `description-pack` combined reseal at HEAD. The measured receipt is the compiler in the `6b3a92c2` archive.
