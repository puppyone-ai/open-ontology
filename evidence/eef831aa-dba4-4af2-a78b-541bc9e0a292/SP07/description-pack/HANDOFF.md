# description-pack handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a292`. Sole coder, tester, and reviewer for this child. No subagents.

## Owned paths

- `tools/description-pack/**` at commit `5ce8b1c4186eb80f9b876a306ae94309f10933ae`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a292/SP07/description-pack/**`

The root workspace, root lock, `tools/description-review`, and `crates/` are not this unit's edits. Fixed archive `c8588faab992af1193ff58dbc489a0b117796aa0`.

## What the bridge does

`description-pack apply` reads one `oo.builder.description-pack-command/v1` document. It re-executes `apply_description_review` and checks `expected_review_sha256`. It recompiles the sealed captured draft through `compile_captured_pg_catalog`. Approved label, description, and grain are resealed by a second compiler call whose dataset fingerprint comes from that compiler. Then `compile_reviewed_effective` and `compile_role_pack` run. See `CONTRACT.md` and `FINDINGS.md`.

## Proof

Decisive container: `linux/proof-3`, `oo-a292-proof3`, id `f637a03cd7a7b8d6648e40dd3e8007b5df160a17e79052194202c2ced2287de5`, exit 0. Smoke 16, pack tests 8, Clippy `-D warnings` clean. `linux/proof-1` and `linux/proof-2` are preserved failures. Do not overwrite their `console.log` files.

A later run needs a new evidence directory and a new container name. Reuse of `oo-a292-eef831aa-cargo` and `oo-a292-eef831aa-target` is only valid while those volumes still exist. `CLEANUP.txt` records their removal after proof-3. The pinned image is local: `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Do not pull. Do not start PostgreSQL. One rust-builder for this task, 6 GiB, 1 CPU. `cargo update` and `cargo generate-lockfile` are out of scope.

## Reviewer notes

The pre-overlay checker is the a288 `lock_subset.rs` mounted read-only (path count 4). The post-overlay checker is `tools/lock_subset.rs` here (path count 5, registry 117). `origin/recipes.txt` cites the a204, a200, a199, and a288 origins.

`MANIFEST.sha256` is the a199 tool's second pass over this evidence directory. It does not include itself.

Parent acceptance is false. SP07 and SP08 stay open. This handoff does not publish a semantic layer and does not grant database roles.
