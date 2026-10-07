# Findings

Verdict: **APPROVE_LIMITED** of original E47. `parent_acceptance` is false. The verdict covers the frozen E47 card. It does not approve a narrower substitute.

## E47 card

The frozen card in `specs/open-ontology-acceptance-counting.md`, `specs/open-ontology-development-ledger.md`, and `specs/open-ontology-v0.1-plan.md` requires `tools/pg-builder` and `testing/real-db/pg-builder`, a real S0/S1 catalog and PostgreSQL profile, approved table cards, an actual Builder LLM, schema-validated persisted S5 table and column candidates, explicit S9/S10 review, an actual role pack, a 100000-row fixture, an actual model call, an input hash, a durable cache, metered tokens, a declared cost or `unknown`, and model failure with zero review or publication side effects. a328 is isolated at baseline `8c2005d5d3cc48eecfb314aa278f7b1d4f00bef8`. Existing compiler and provider production contracts stay unchanged. Spec row SP07-W04 remains the S5 LLM JSON, residency, injection, and cache-budget row. Closing E47 does not close SP07.

Held on this run, with the a328 receipts mounted read-only and hashed before and after:

- Catalog, profile, cards, and seal hashes match the stored 100000-row a328 capture. `testing/real-db/pg-builder` is tree `17668deacecdd51215352f10cb9f828a9dbb2f7e` at both `a13b1cc` and `be9acbc23`. PostgreSQL was not started again.
- Three pass-1 generations are `validated`, model `grok-4.7`, socket `opened`, HTTP 200, with the input hashes, request hashes, and token counts in `REPORT.md`. Declared cost status is `unknown`.
- Cache files are `{input_sha256}.json` and carry the same candidate as the generation. The directory digest is unchanged by the pack call.
- S9 operator review and S10 classifications are the stored files. The combined command does not write `review.state`. Effective column `review` is null while the approved description is present.
- One `pack_combined` result is protocol `oo.pg-builder.pack-combined/v1`, outcome `packed`, `reseal_count` 1, two role packs, and a resealed wrapper hash different from the original wrapper hash. The narrow role retains the approved party and account sentences. Entry stays deferred and out of the role text.
- Timeout, unavailable, and the pass-2 structural draft refuse review and pack. Fixture failure directories keep their digest and gain no cache file.
- `generate.rs`, `cache.rs`, `rebind.rs`, `http.rs`, and `tests/contract.rs` match the a357 consumer blobs. `description-pack` `tests/pack.rs` matches `a13b1cc` and was not rerun. `apply.rs` adds `pub(crate)` on the existing helpers and adds `occupied_replacement_values`.

## Combined product

`pack_combined` merges the explicit per-table reviews that share one schema hash, evidence hash, curated hash, and prior compiler-review CAS, then calls the existing reseal once, then compiles one two-role pack. Per-table `pack_pair` still reseals one generation. On the stored documents the party pack contains the party sentence and omits the account sentence. The account pack does the reverse. Those per-table files are the retained old behavior. Their protocol differs from `oo.pg-builder.pack-combined/v1`. Joining their role texts in either order differs from the combined narrow role text.

The stored narrow role keeps both exact approved bodies behind the `UNREVIEWED:` prefix because `include_unreviewed` lists `column_descriptions` and the compiler review state is unset. A second call that omits `column_descriptions` omits those bodies and still reports `reseal_count` 1 for that separate command. Hidden `national_id` is recorded absent. The restricted role omits the display-name and phone sentences. The curated file bytes and the stored generation objects, including `complete_command` and the dataset-only seam id, are unchanged. Party rebind is bound, model bytes are copied, the stored generation is unchanged, and the rebound prompt is `oo.builder.description-prompt/v2` against the stored `oo.builder.description-prompt/v1` command.

The independent fixture uses a369 table names and body strings. It is generated through the builder seal, card, curated scaffold, and generate path. Injection text appears in a posted payload and stays out of `PROMPT` and out of the role pack.

## Refusals

`pack_combined` derives its pins from the reviews, so a consistent call cannot be stale. The stale clause is the callee `apply_description_pack_combined` with a zero expected schema hash. The code ends in `/review.stale`. Duplicate dataset, conflicting classification JSON, and a failed model go through `pack_combined`. Mixed scope of an aggregate plus another file is raised by the builder as `oo.pg-builder/v1/review.mixed_scope`. The same code refuses a fixture shelf generation mixed with the stored party generation. None of these paths change the curated canonical digest, the receipt file hashes, or the cache directory digest.

## What stays open

SP07 stays open. Public authentication stays open. Semantic publication stays `not_claimed`. Publication stays `unavailable`. Grants stay `not_authorized`. Release stays open. `parent_acceptance` is false. The two extra cache files named in `REPORT.md` are different candidates and were not packed. Numeric oid 1700 is a separate seam. This review does not edit the ledger checkbox.
