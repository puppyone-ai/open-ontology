# Handoff

Verdict: **DELIVERED_FOR_A328**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a339`. Producer commit `c23db28dc27f5cf20741dd36e8a8e88129ad1715`. Parent `93f5b8aea78d0d745334da0bc5352afdb86412e2`. Fixed archive `9cdbc625d02fca2fc3fd31b62ce8cb03692a9b81`, tree `a78e925e6cd8b0914702c80d9a65f76c5fc1185f`. Decisive run `linux/proof-7`, container id `02949443c871a986285e9a6cb82836d4cc53a05a2d3c31aa7d431f263a02e744`, `docker-run.exit` 0.

This file is the producer handoff for S9 and the S10 two-role pack. It does not close E47. It does not accept the parent goal. It does not add a countable unit. It does not certify a release. a328 commit `149944271ef1c2cb555954d65248413df2166f1c` remains the S5 consumer that asked for this API. That commit is not SP07 acceptance.

## Read first

- `REPORT.md` and `FINDINGS.md`
- `source/blobs.txt` and `linux/proof-7/overlay.sha256`
- `FREEZE.md` for the frozen column schema
- `linux/smoke-3/` for the harness panic, and `linux/smoke-4/` for the pre-edit provider pass
- `linux/proof-1/` through `linux/proof-6/` for the kept failures
- `linux/proof-7/` for the 0 exits, Clippy, and lock subset
- `origin/recipes.txt`, `CLEANUP.txt`, `MANIFEST.sha256`

## Decisive exits

`linux/proof-7/docker-run.exit` 0. Every phase file under `linux/proof-7/phases/` is 0, including `clippy-pack.exit`, `clippy-provider.exit`, `lock-subset.exit`, and `root-lock.exit`. Counts: candidate 10, CLI 5, lib 2, review 21, pack 21, provider 23. Lock summary `registry_triples=117 missing=0 mismatch=0 path_packages=5 direct_pins_present=true`. Memory `6442450944`, NanoCpus `1000000000`, CapDrop `["ALL"]`, Privileged false, OOMKilled false, NetworkMode `oo-a339-eef831aa-net`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Rust 1.94.0 aarch64. PostgreSQL was not started.

`linux/proof-7/capacity.txt` recorded `own=0 global=0 host_cargo=0` before start. The launch script removed `oo-a339-proof7`.

## Immutable API

Consume producer commit `c23db28dc27f5cf20741dd36e8a8e88129ad1715`. The ten owned blobs are listed in `source/blobs.txt` and match `linux/proof-7/overlay.sha256`.

v1 stays on these protocols when `prompt_version` is `oo.builder.description-prompt/v1`:

- `oo.builder.description-prompt/v1`
- `oo.builder.description-request-command/v1`
- `oo.builder.description-request/v1`
- `oo.builder.description-response/v1`
- `oo.builder.description-candidate/v1`
- `oo.builder.description-review-command/v1`
- `oo.builder.description-review/v1`
- `oo.builder.description-pack-command/v1`
- `oo.builder.description-pack/v1`

v2 is selected only by `binding.prompt_version` = `oo.builder.description-prompt/v2`. Any other version is `version.unsupported`. Prompt SHA-256 `ec5355303ad18ce189e9a297677a55f9f6317e1ab59d553b057d05c81f6278c2`. Response protocol `oo.builder.description-response/v2`. Outcome protocol `oo.builder.description-candidate/v2`. Review protocol `oo.builder.description-review/v2`. Pack protocol stays v1. Compiler id stays `oo-builder/description-candidate/v1`. Review id stays `oo-builder/description-review/v1`. Pack id stays `oo-builder/description-pack/v1`. The v2 constants are not re-exported from `oo_builder`. Callers use the protocol strings.

A v2 column object on the response has exactly `dataset`, `column`, `attnum`, `card_sha256`, `label`, `description`, `confidence`, `caveats`, `open_questions`. `column` is the description-request card’s physical name. `card_sha256` is that description-request card hash. It is not the pg-builder evidence card hash. The compiler adds `ident` from the card and `effect` `review_only`. The set must be every physical column of every selected visible table. A missing, extra, or duplicate pair, or a wrong `attnum`, is `incomplete` with both arrays empty. An unknown key is `format_rejected` with both arrays empty. Comments stay `{byte_len, sha256}` or null.

A column decision carries `attnum`, `candidate_sha256`, `card_sha256`, `column`, `curated_sha256`, `dataset`, `decision`, `evidence_hash`, `request_sha256`, `reviewer_at`, `reviewer_label`, `schema_hash`. `decision` is `approve`, `reject`, or `defer`. `fields` is present only on approve, and only `label` and `description` are legal there. `candidate_sha256` is the canonical hash of the compiler column object. `curated_sha256` is the canonical hash of the whole `DatasetFile` before the write. `current_sha256` for a field is the canonical JSON string, or the four bytes `null` when the text is absent. The stored column record sets `authenticated` false and `confidence_effect` `recorded_only`, and it keeps candidate confidence, caveats, and open questions on that record.

Pack command fields `replace_existing_column_review` (default false) and `expected_existing_column_review_sha256` (default absent) cover keys `column/{qualified}/{ident}`. Dataset occupied CAS is unchanged. `reject_stale` still fails the pack when any queue item has kind `stale`.

Role packs: put `column_descriptions` in `settings.ontology.publish.include_unreviewed` when the caller wants unreviewed column description text in the pack. Labels render for columns the role can see. This seam does not forge `column.review`. Hidden, unknown, and personal-disallowed columns stay out of the role pack, including their text and field metadata.

## a328 integration

Keep the v1 table response path. For columns, build one captured body whose protocol is `oo.builder.description-response/v2`. Set the description command `prompt_version` to `oo.builder.description-prompt/v2`. Send one column object for every physical column on the selected cards. Use the description-request card name as `column` and the description-request card hash as `card_sha256`. Rebind before validation: a328 `validate_fields` still expects nine keys and treats `card_sha256` as the evidence card hash.

Pass S9 `column_decisions` from existing authorized operator review decisions, with the six hashes bound to this candidate, this source catalog, this input, and the prior review CAS. Pass S10 `pii` and `semantic_type` only as existing pack decisions into `compile_reviewed_effective`. Leave `deadline_ms` where it is. A timeout is not a validated candidate. Store nothing from a truncated, incomplete, or provider-failure body.

Do not edit this producer to match a moving pg-builder tree. The consumed source is `c23db28dc27f5cf20741dd36e8a8e88129ad1715`.

## Re-run

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `RUSTUP_TOOLCHAIN=1.94.0`. `HOME=/tmp`. One builder, 6 GiB, 1 CPU, `--cap-drop ALL`, `--pull never`. `git archive 9cdbc625d02fca2fc3fd31b62ce8cb03692a9b81` of `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates`, `tools/description-review`, `tools/description-pack`, `tools/description-provider`, and `contracts/sdk/json-query/fixtures/tls`. Mount it read-only. Move `rust-toolchain.toml` aside. Overlay only the ten files in `linux/proof-7/overlay-files.txt` from the producer commit. Export `OO_A263_DESCRIPTION_CANDIDATE` to the compiled binary before the provider tests. Keep the committed locks. Do not run `cargo update` or `cargo generate-lockfile`. Do not start PostgreSQL.

A later run needs a new evidence directory and a new container name. `CLEANUP.txt` records removal of this actor’s volumes and network.
