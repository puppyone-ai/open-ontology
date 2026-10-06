# S5-D02 description-candidate CLI

## Scope and verdict

This child adds a compiled stdin CLI for the public description-candidate API. The operator passes `prepare_request` or `complete_candidates`. The process calls `prepare_description_request` or `complete_description_candidates` and writes that function's canonical document to stdout.

The command does not call a model, open a network connection, read credentials, use PostgreSQL, write curated or effective text, grant, certify, authenticate, or publish. A hand-captured response is caller input. Candidate `effect` stays `review_only`. Confidence stays recorded data.

This is not full S5. This report does not claim parent acceptance, publication, or release.

Contract commit `2abc6c1a14401ce1b2165090dbaef9f4aba8bec5`. Implementation commit `827e768b9f94af6134a5734bd16e066ab0d47528`, whose parent is the contract commit.

## Fixed source

Fixed commit `fbc10b04017bd7e0eb922c3c60732bbdf9a7f46a`. The product proof builds a git archive of that commit and overlays only:

- `crates/oo-builder/src/bin/description-candidate.rs`
- `crates/oo-builder/tests/description_candidate_cli.rs`
- `contracts/builder/description-candidate-cli-v1.md`

The archive listed and extracted 43045 files. `Cargo.lock` stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. No manifest, lock, library, or other bin was edited. a207 remains the connector manifest and root lock writer.

The CLI contract was committed before the binary and the test.

## Recipes

Launch and report shape follow reviewed recipes, with the UUID, fixed commit, and owned paths changed:

| Origin | Commit | Path | Blob |
|---|---|---|---|
| a204 preoverlay launch | `b2b3adabd05de94723d8ed13b33be0144a0de1f9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` | `3737c1a8321814ff3b1070ddf8b89d6864c63f6f` |
| a253 preoverlay launch | `268e46972458fe53dd54412ab277b2cd09552a82` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a253/SP07/description-candidate/linux/preoverlay/launch.sh` | `64691d41f933927ff537c0cfe413748b08d158d3` |
| a253 preoverlay inside | `268e46972458fe53dd54412ab277b2cd09552a82` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a253/SP07/description-candidate/linux/preoverlay/inside.sh` | `4aecdc35b11e08e8c9a010f6ccf0fc6c4cefa5d2` |
| a253 proof-7 launch | `268e46972458fe53dd54412ab277b2cd09552a82` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a253/SP07/description-candidate/linux/proof-7/launch.sh` | `c14c7146fe73f913d1e22efe8b605107ab56b07d` |
| a253 proof-7 inside | `268e46972458fe53dd54412ab277b2cd09552a82` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a253/SP07/description-candidate/linux/proof-7/inside.sh` | `9f15b1b72a8f53f045107a42d6a977d90d75cde1` |
| a199 manifest tool | `5b8cce62cf51e42d9029f13371da4bb4ee8cdaa9` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/tools/sha256_manifest.rs` | `f21989651570187fc286462349d59174a6a6b8ef` |
| a253 file hasher | `268e46972458fe53dd54412ab277b2cd09552a82` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a253/SP07/description-candidate/tools/sha256.rs` | `b69a2af13099ce4baf2d04e79c5692d9988ab67d` |

`tools/sha256_manifest.rs` and `tools/sha256.rs` are byte-identical to those blobs. a200 commit `a587031c719a484830d78e493605f48e2056cae4` is the private PostgreSQL recipe. This unit starts no database, so that recipe was not applied.

## Pre-edit smoke

`linux/preoverlay` on the pinned Linux arm64 Rust 1.94 image, 6 GiB, 1 CPU, private network, dropped capabilities, no pull. The archive contained the description-candidate library and did not contain the CLI bin, the CLI test, or the CLI contract. `overlay-absent.txt` says `cli_overlay_absent`.

- `description_candidate::tests`: 2 passed
- `tests/description_candidate.rs`: 10 passed
- `cargo clippy -p oo-builder --lib --test description_candidate -- -D warnings`: exit 0
- lock unchanged
- `smoke.exit` 0, `docker-run.exit` 0, OOMKilled false, privileged false

`rustc` reported `rustc 1.94.0 (4a4ef493e 2026-03-02)`. Memory was 6442450944 and NanoCpus was 1000000000.

## Decisive verification

`linux/proof` is the passing run of commit `827e768b9f94af6134a5734bd16e066ab0d47528`. Same image, 6 GiB, 1 CPU, offline fetch, fixed archive plus the three-file overlay. The tree diff is only those owned paths and the toolchain file moved aside on the writable copy.

- `tests/description_candidate_cli.rs`: 5 passed, 0 failed
- `cargo clippy -p oo-builder --bin description-candidate --test description_candidate_cli -- -D warnings`: exit 0, no warning lines
- lock unchanged
- `smoke.exit` 0, `docker-run.exit` 0, OOMKilled false, privileged false

Content SHA-256 of the proved overlay matches the committed files:

| File | SHA-256 |
|---|---|
| `crates/oo-builder/src/bin/description-candidate.rs` | `c28297d86349507fff5bc67e5aa796a0f58836b8974cc9b1d7f08ade7d31798d` |
| `crates/oo-builder/tests/description_candidate_cli.rs` | `96e0ab7ddb35280ef6eb3deee8b8799bc386e87042070b3d493941e5bead0c9d` |
| `contracts/builder/description-candidate-cli-v1.md` | `bd37f499bbba86dae2905bc45e1770a5342193f8facc7d62dcd34458f3156d64` |

Git blob ids for the same bytes are `273de1e7cba3835150766545ba320320f56e65eb`, `a4f1f7377ce2dcc47d3c9d882bebeefc47d2218a`, and `1d6f461677a32705a7463c153ae15fecb109b77a`. `linux/proof/overlay-blobs.txt` records those ids.

The shop fixture is compiled with `compile_captured_pg_catalog`. Prepare stdout equals `prepare_description_request`. A hand-built `oo.builder.description-response/v1` body is then passed to `complete_candidates`, and that stdout equals `complete_description_candidates`. Accepted candidates have `effect=review_only`. A label equal to the injection sentence stays a label. Statements keep `publication=unavailable` and `llm_transport=not_used`.

Hidden visibility, a 31-byte schema with no generated dataset, and a stale evidence hash exit 1 with an empty stdout. Stderr is `description-candidate: <code> <message>` and does not include the owner, the schema text, or the stale hash. Malformed JSON, a duplicate key, a non-UTF-8 buffer, and a 1048577-byte stdin exit 1 the same way. `incomplete`, `truncated`, a one-dataset complete body, and a response over `max_response_bytes` exit 0 with `candidates=[]` and the sealed structural hashes. The in-memory draft string is unchanged.

## Retained failures

The preoverlay smoke and the proof each passed on the first launch. No earlier proof directory was replaced.

## Limits

The CLI does not reimplement seal, selection, budget, or response checks. Column-level candidates, samples, comment bodies, model transport, residency approval, and token cost remain outside this unit. A phase argument is required. The function names are not accepted as phases.

## Reproduction

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL was not started. Launch scripts are `linux/preoverlay/launch.sh` and `linux/proof/launch.sh`.

## Cleanup

`CLEANUP.txt` records removal of the a258 cargo volume, target volume, private network, and `/tmp/oo-a258-eef831aa-dba4-4af2-a78b-541bc9e0a258`. No prune and no foreign cleanup.

## Inventory

`SHA256SUMS` lists every regular file in this directory except itself. The hasher is `tools/sha256_manifest.rs`. `linux/manifest/launch.sh` compiles it with `rustc --edition 2021 -D warnings` on the pinned image, 6 GiB, 1 CPU, `--network none`. `linux/manifest/finalize.sh` rewrites `SHA256SUMS` after the manifest console and exit files are closed.
