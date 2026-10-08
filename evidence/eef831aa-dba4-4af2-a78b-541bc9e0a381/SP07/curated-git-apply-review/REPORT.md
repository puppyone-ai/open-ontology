# Curated Git apply review

Verdict: APPROVE_LIMITED

The fixed adapter at `576e476f10eb6c9e3a06bdde7cc4f9e8b17bc9f6` meets original E57-D1/1 as an operator boundary. An approved description review, re-executed by the pinned compiler, plus an expected head and blob, becomes one commit on `refs/heads/curated` and one receipt. The receipt says `authenticated: false` and `parent_auth: required`. This acceptance stops at that operator adapter. Public B04 identity and semantic publication stay outside it. This is not parent SP02 or SP07 acceptance, and it is not goal acceptance.

## Freeze and contract

Ledger, plan, and acceptance-counting share the 2026-10-08 E57-D1/1 sentence: an approved typed review command plus the current curated/config Git head, blob, and evidence is re-run through the reviewed compiler into one private Git object-tree commit, an old-SHA compare-and-swap, and an idempotent receipt. The worktree and the shared index stay untouched. Roles, secrets, and unapproved items are not written. a374 is only `tools/curated-git-apply` and its own evidence. Original B04 API, identity, and public Git app integration remain unfinished.

Contract commit `9d33bfc065901db78e8f736e3a840b57630a31e4` and the fixed commit have the same `CONTRACT.md` blob `fd8162d95d717fbc65a9fc12874d3f0d92081169`. `9d33bfc065901db78e8f736e3a840b57630a31e4` is an ancestor of the fixed commit. The product tree is `4b5b6477846aa4c2883898bddfb723f1b8218f57`.

## Pins checked on the host

`git rev-parse` at the fixed commit, recorded in `linux/proof-2/pins.txt`:

- compiler pin `be9acbc23a9e4c477b9cf6bd4b19ad22fad3abba`
- `crates/oo-core` `769dd0e8f7ee77a29017fb3e0f9d90917b0a8194`
- `crates/oo-builder` `40f74d7e9aac9c13e95d1da3d61d689184773c77`
- `crates/oo-jobs` `ca228d7c648e554675cdf31ae828320e9d05cd9e`
- `tools/description-review` `a60b6d82e5b114430a66d82f91de4d3a7f3647c4`
- `tools/description-pack` `9d99fe90ecf87c09b4bb33e46e89bd078c10586b` (recorded, not linked)

The archive has no `.git`, so `pinned_trees_match_the_reviewed_commit` was skipped. The host pins above are the check.

## How the proof was built

Smoke, then one proof container, then a second proof container after the first had been removed. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, linux/arm64, not pulled. `RUSTUP_TOOLCHAIN=1.94.0`. Each container was `--cap-drop ALL`, not privileged, 6 GiB, 1 CPU. Smoke used `--network none`. The proof used the private bridge `oo-a381-eef831aa-net`. The source mount was read-only. Cargo home and the target dir were private volumes. `Cargo.lock` hash `5c7d530fe87af07175734644cfc9d9246332a4c349e10a62ee95c581686f3c30` was unchanged. No Postgres.

Smoke `oo-a381-smoke` exit 0: rustc 1.94.0 (4a4ef493e 2026-03-02), cargo 1.94.0, git 2.47.3, aarch64.

`oo-a381-proof` exit 101. The review oracle did not compile (`shell_single` received a `PathBuf`). That directory is kept. The product sources were not edited.

`oo-a381-proof2` reused the same private volumes after the first container was gone. Offline fetch exit 0. Clippy component exit 0. `cargo clippy --locked --offline --all-targets -- -D warnings` exit 0. Host config: `OOMKilled=false`, `Privileged=false`, `CapDrop=["ALL"]`, `Memory=6442450944`, `NanoCpus=1000000000`, source `rw=false`.

## Independent result

`tests/a381_review.rs` is not in the fixed commit. It was copied onto the extracted archive and calls the product CLI and `apply_curated_git`. Candidate text is hand-typed and passed through `compile_captured_pg_catalog`, `prepare_description_request`, and `complete_description_candidates`. The fixture is a private repository, billing invoice and payment, not the author's public catalog.

`test result: ok. 4 passed` in `linux/proof-2/test.stdout`:

- `descriptive_commit_preserves_semantic_fields_and_unrelated_tree`
- `replay_conflict_and_lost_reply_do_not_duplicate`
- `two_writers_one_cas_winner`
- `dirty_worktree_and_index_stay`

Those runs checked a flipped blob compare-and-swap (ref and index stay, exact `blob.cas` stderr), then a commit that changes invoice label, description, and grain only. Payment, roles, generated ontology, the entity file, the hook file, `oo.yaml`, and the secret ref keep their blobs. The worktree bytes, `.git/index`, and `.git/config` stay. The receipt has no extra newline, `authenticated` false, `parent_auth` required, `identity` declared_only, and the service author `oo-curated-git-apply <curated-git-apply@open-ontology.invalid> 1791417600 +0000`. The reviewer label is not in the commit body. Replay returns the same receipt. A same key with a different column list returns `idempotency.conflict`, leaves the ref, and leaves the losing commit object. A later unrelated tip still returns that receipt and does not move the ref. Two writers: one `Ok`, one `ref.cas`, parent of the winner is the original head, one unreachable commit remains. A dirty `roles.yaml` is still dirty after the commit, the committed roles blob is the clean blob, and the shared index is unchanged. The Git trace allow-list is the plumbing commands only. Hook markers stayed absent. The child environment did not carry the hostile parent SSH socket, SSH command, host author, or host gitconfig.

Library `json::tests::duplicate_key_and_trailing_data_fail` passed (1). The binary crate had 0 unit tests.

## Author suite on this archive

`cargo test` exit 101 because `tests/apply.rs` requires the source checkout `.git`. Nine tests panic at `source git dir`. `cli_usage_and_unknown_field` passed. `pinned_trees_match_the_reviewed_commit` was filtered. The accepted-review test also re-execs itself and that child exits 101 for the same missing `.git`. This is a property of the immutable archive, not a product failure. a374 remains the author's report of that suite on a real checkout. It is corroboration. It is not this verdict.

## Author package

Evidence commit `351adf350b0901312e6e2d77e10c8debfd272e49` contains `REPORT.md`, `FINDINGS.md`, `HANDOFF.md`, `failed-attempts.md`, `source-inventory.txt`, and four files under `raw/`. It has no `SHA256SUMS`. This review does not invent one. `source-inventory.txt` here is a fresh `git ls-tree` of the fixed commit. The blob ids match the names in the author's inventory. The bytes were not copied from that file.

## Scope

`description-pack` is pinned and not called. Column v2 label behavior was not re-run here, because the author test that covers it stops at the missing source `.git` before any column assertion. The root manifest and root lock were not modified. No customer call, no push, no publication, no grant.
