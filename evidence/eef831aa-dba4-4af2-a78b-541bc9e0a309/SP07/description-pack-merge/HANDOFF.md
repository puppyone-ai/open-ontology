# description-pack occupied-review handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a309`. Sole coder, tester, and reviewer for this extension. No subagents.

## Owned paths

- `tools/description-pack/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a309/SP07/description-pack-merge/**`

Contract commit `c548c8b18c4de5d75e6f8527c71569a2c311eb11`. Implementation commit `ceb4be5ebe12cf581f511cc8a676050d02465c55` changes `src/apply.rs` and `tests/pack.rs`. The root workspace, root lock, `tools/description-review`, and `crates/` are not this unit's edits. Fixed archive `cab5e8e219d1924070df7336f5ab5d4ce7cf40dc`.

## What the bridge does

`description-pack apply` still reads one `oo.builder.description-pack-command/v1` document. Descriptive approval after a bound dataset review requires `replace_existing_dataset_review` true and `expected_existing_review_sha256` equal to the canonical hash of that stored four-field review. Only label, description, and grain are written onto the curated entry. The compiler review array is passed through `compile_captured_pg_catalog` unchanged. The pack outcome carries `occupied_replacements` with bridge provenance, the prior review, both review hashes, and the description decision's reviewer label, date, and authenticated flag. See `CONTRACT.md` and `FINDINGS.md`.

Without the flag, a bound dataset review remains `unsupported` / `review.dataset_decision_occupied`.

## Proof

Decisive container: `linux/proof-1`, `oo-a309-proof1`, id `00fae351c38111f800e1b608ab4bd87465525a7121b74ef995deccf355d947b0`, exit 0. Archived pack tests 8, overlaid pack tests 14, Clippy `-D warnings` clean, a302 boundary oracle 7. No earlier proof directory.

A later run needs a new evidence directory and a new container name. `CLEANUP.txt` records removal of `oo-a309-eef831aa-cargo`, `oo-a309-eef831aa-target`, and `oo-a309-eef831aa-net`. The pinned image is local: `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Do not pull. Do not start PostgreSQL. One rust-builder for this task, 6 GiB, 1 CPU. `cargo update` and `cargo generate-lockfile` are out of scope.

## Reviewer notes

`tools/lock_subset.rs` is the unchanged a292 path-count-5 tool. `tools/sha256_manifest.rs` is the unchanged a199 tool. `origin/recipes.txt` cites a204, a200, a199, a288, a292, and the a309 script hashes.

The a302 file `overlay/a302_sealed.rs` was executed from a read-only mount. It is not in the product tree.

`MANIFEST.sha256` is the a199 tool's second pass over this evidence directory. It does not include itself.

Parent acceptance is false. SP07 and SP08 stay open. This handoff does not publish a semantic layer and does not grant database roles. The same unit may still need an amended review. This delivery does not add a denominator child.

The mixed occupied-and-fresh reseal branch is implemented and is not covered by its own test. The fresh-only path is the original approve test. The occupied-only path is `occupied_override_approve_reseals_two_roles`.
