# S5-D01 description candidates

## Scope and verdict

This child implements the frozen description-candidate unit. One explicit caller-admitted sealed captured draft, the full selected visible table identities and evidence hash, and a bounded description budget become a deterministic description request. One complete captured model JSON response then becomes schema- and evidence-bound generated candidates: label, description, grain, caveats, and open questions.

The compiler does not call a model, open a connection, read a credential, attach a sample, grant, certify, or publish. Candidate confidence is stored and does not approve a row or classify PII. Curated, effective, grants, and publication are not written.

This is not full S5. Model transport, residency approval, and cost are a later child. This report does not claim parent acceptance or release.

Source commit `fbc10b04017bd7e0eb922c3c60732bbdf9a7f46a`.

## Fixed source

Fixed commit `3f8807b495bd78ee1d8c99aca41c54c3137778c6`. The product proof builds a git archive of that commit and overlays only:

- `crates/oo-builder/src/description_candidate.rs`
- `crates/oo-builder/src/lib.rs` (module export only)
- `crates/oo-builder/tests/description_candidate.rs`
- `contracts/builder/description-candidate-v1.md`

`Cargo.lock` stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. No manifest, lock, or bin edits.

Contract `contracts/builder/description-candidate-v1.md` was written before the module. Public entry points are `prepare_description_request` and `complete_description_candidates`.

## Pre-edit smoke

`linux/preoverlay` on the pinned Linux arm64 Rust 1.94 image, 6 GiB, 1 CPU, private network, dropped capabilities, no pull. The archive did not contain the new module. `cargo test -p oo-builder --lib captured_pg`, `cargo test -p oo-builder --test captured_pg`, and Clippy `-D warnings` on that surface exited 0. `smoke.exit` is 0. `docker-run.exit` is 0. The lock hash matched the archive.

## Decisive verification

`linux/proof-7` is the passing run of the committed bytes. Same image, 6 GiB, 1 CPU, offline fetch, fixed archive plus the four-file overlay. The tree diff is only those owned paths and the toolchain file moved aside on the writable copy.

- `description_candidate::tests`: 2 passed
- `tests/description_candidate.rs`: 10 passed
- `cargo clippy -p oo-builder --lib --test description_candidate -- -D warnings`: exit 0
- lock unchanged
- `smoke.exit` 0, `docker-run.exit` 0, OOMKilled false, privileged false

Content SHA-256 of the proved overlay matches the committed files. Recorded in `linux/proof-7/owned-sha256.txt`.

The shop fixture is compiled with `compile_captured_pg_catalog`. The email comment is the SHA-256 of the injection sentence, byte length 58. The sentence is absent from the catalog and from the request. The hash sits under `boundary.data`. A candidate label that repeats the sentence stays a label. Statements keep `publication=unavailable`, `auto_accept=not_authorized`, `pii_classification=not_performed`, and `confidence_effect=recorded_only`. Each accepted candidate has `effect=review_only`.

Selecting only `public.customer` produces a request whose bytes do not contain `purchase`. Format, truncation, incomplete, provider, and budget failures keep the structural draft hashes and return no candidates.

## Retained failures

| Run | Exit | Cause |
|---|---|---|
| `linux/proof` | 2 | `diff -rq` sort order did not match the expected file. No compile. |
| `linux/proof-2` | 101 | `usize::from(u32)` rejected by rustc 1.94 (`E0277`). |
| `linux/proof-3` | 101 | Tests passed. Clippy `large_enum_variant` on `Stage::Ready`. |
| `linux/proof-4` | 101 | Tests passed. Clippy `useless_format`. |
| `linux/proof-5` | 101 | Tests passed. Clippy `needless_borrow`. |
| `linux/proof-6` | 0 | Passed before the 17-row `budget.tables` assertion was added. |

Raw logs, exits, and overlays for those runs are kept. `linux/proof-7` is the run of the committed test file.

## Limits

The request carries structural table cards and comment hashes. It does not carry comment bodies, samples, owners, privileges, or credentials. There are no column-level candidates and no CLI. A selection above 16 tables is `budget.tables` and was exercised with 17 copies of one admitted row. Cards are not truncated to fit a byte budget.

The catalog parser rejects `primary_key_nulls_not_distinct=false` when the primary key is empty. The hidden 31-byte schema fixture therefore uses null. Its schema hash is `49c560bd3d46e627813a8352a165fd042a1fbce1acbf7b51979ace0b97f330f2`. The schema ident is 64 characters and the structure draft does not emit a generated dataset. Selecting it returns `selection.hidden` without echoing the schema or the owner.

## Reproduction

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Rustc reported `rustc 1.94.0 (4a4ef493e 2026-03-02)`. PostgreSQL was not started. Launch scripts are `linux/preoverlay/launch.sh` and `linux/proof-7/launch.sh`.

## Cleanup

Own containers were removed by each launch. After proof-7 the cargo volume, target volume, private network, and `/tmp/oo-a253-eef831aa-dba4-4af2-a78b-541bc9e0a253` were removed. `CLEANUP.txt` records the commands. No prune and no foreign cleanup.

## Inventory

`SHA256SUMS` at the root of this directory lists every regular file here except itself. The hasher is `tools/sha256_manifest.rs`, the a199 std-only tool, content SHA-256 `d2172fbd1c61d9ad869e2f596c4c2a35334e9f877df102e67e17d720e4ef8eb3`. `linux/manifest/launch.sh` compiles it with `rustc --edition 2021 -D warnings` to `/usr/local/bin/sha256_manifest` on the pinned Rust 1.94 image, 6 GiB, 1 CPU, `--network none`. That first walk is intermediate: its stdout is `linux/manifest/console.log`, and the exit files are written after it. `linux/manifest/finalize.sh` runs the same binary again and rewrites `SHA256SUMS` after `console.log`, `hostconfig.txt`, and the exit files are closed. The rewrite stdout stays outside this directory. The `files=` line in `console.log` is the earlier walk.
