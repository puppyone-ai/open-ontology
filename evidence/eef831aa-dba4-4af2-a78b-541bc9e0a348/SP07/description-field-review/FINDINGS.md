# Findings

Verdict: **APPROVE_LIMITED** for the column-field producer seam at `c23db28dc27f5cf20741dd36e8a8e88129ad1715`.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a348`. Decisive evidence is `linux/proof-3`. The oracle is `overlay/a348_sealed.rs`, content SHA-256 `515edffdea475fd38fa2e110e3c00b142dfb3813c589b8547340ad1131d68c63`. It is a fixture test. It does not open PostgreSQL and does not call a model.

E47 is not accepted. SP07 is not accepted. The parent goal is not accepted. No count changes. Builder integration of this producer is not part of this verdict.

## Held by the independent oracle

`explicit_column_review_reseals_two_roles_without_forging_review` held the v2 card, the typed write, and the two role packs.

- The v2 request prompt SHA-256 is `ec5355303ad18ce189e9a297677a55f9f6317e1ab59d553b057d05c81f6278c2`. The request text does not contain `sample_values`.
- The outcome has five columns. The crate column's `attnum`, `card_sha256`, and `ident` come from the description-request card. `card_sha256` is not the evidence hash. `effect` is `review_only`. The column object has no `pii` and no `review`.
- Outcome statements are `pii_classification` `not_performed`, `auto_accept` `not_authorized`, and `confidence_effect` `recorded_only`.
- Review protocol is `oo.builder.description-review/v2`. The crate decision is applied, `authenticated` false, confidence recorded only, and both dispositions `replaced`. Caveats and open questions are queue items of kind `pending`.
- The written dataset column receives the new label and description and keeps the prior `pii`, `status`, classification, and review object. Confidence, caveats, and open questions are not dataset keys. The badge column receives text, keeps `pii` `restricted`, and has no review object. Dataset caveats and the dataset label stay. `public.quay` is unchanged.
- Pack outcome is `packed`. The reseal statement is `compiler`. `llm` is `not_used`. `source_artifact` is `not_mutated`. The original wrapper recompiles to the same hash. The resealed wrapper matches a fresh compile.
- Without `column_descriptions`, both role artifacts omit the new bodies. North also omits `badge_code` and the hidden bay text. The effective column carries the new description with `review` null. Effective `pii` comes from the caller classification decisions (`none` on the crate column, `personal` on the badge column), not from the curated file and not from the model.
- With `column_descriptions`, south's artifact contains the bodies only behind `UNREVIEWED:`. North still drops the crate body because it names hidden `badge_code`. South includes the personal-sample omission sentence and omits `private_mark`.
- Both role rows match `compile_role_pack`, and the digests differ.
- `descriptions` is `include_unreviewed.unsupported` and the message says it was not treated as a grant. A repeated `column_descriptions` is `include_unreviewed.duplicate`. The pack JSON does not contain confidence `0.25`, the caveat, the question, or `a348_CLASS_DO_NOT_ADMIT`.

`v1_missing_extra_and_failures_write_nothing` held compatibility and zero effects.

- v1 prepare succeeds and uses a different prompt hash. The v1 outcome protocol is `oo.builder.description-candidate/v1` and has no `columns` key.
- A `columns` key on a v1 response is `unknown_key`, with empty candidates.
- A column decision against that v1 outcome is `decision.unknown`.
- Missing, extra, and wrong-`attnum` columns are `incomplete` with both arrays empty. The pack is `not_candidates` and does not reseal.
- A wrong card hash is `evidence_identity`. A model `pii` key is `unknown_key` and drops the canary.
- `provider_failure` emits empty columns and empty candidates. The pack does not reseal. The original wrapper hash is unchanged.

`occupied_cas_stale_defer_and_unknown_pii_fail_closed` held the column CAS and the closed failures.

- The compiler review is four keys: `action`, `fact_fingerprint`, `item_id`, `schema_hash`. It does not contain the reviewer name.
- Flag absent: `review.column_decision_occupied`, no reseal, and the second canaries are absent.
- Wrong hash and an extra key: `review.occupied_cas`.
- A hash without the flag: `schema.invalid`, message `a replacement hash requires authorization`.
- The exact prior hash reseals. Attribution is `bridge_provenance`. Both recorded hashes equal the prior review. Provenance is `nia.declared`, `authenticated` false, fields `description` and `label`. The four-field object is unchanged. The curated review object is still the original approved object.
- A stale schema hash is `review.schema` and the error does not contain the body.
- Defer and reject are `reviewed_unchanged`, reseal `not_required`, and the curated description stays `kept crate note`.
- A CAS map with no occupied approval is `review.occupied_cas`.
- Allowlisting `private_mark` without a classification decision is `classification.unknown_pii`. The error does not contain the hidden body.

Author tests on the same archive, before the oracle file existed, passed at the counts in `REPORT.md`. Those tests include the author's column reseal, occupied column CAS, stale schema, defer, reject, and unknown-PII names. This review did not replace them.

## include_unreviewed is intentional

The publication path for the new description is the explicit token `column_descriptions`. The role compiler then prefixes `UNREVIEWED:`. A curated `review.state` of `approved` does not make that new text approved, because this seam copies label and description onto the captured overlay and leaves effective `review` null.

`docs/06-ontology-layer.md` uses the word `descriptions` in the merge-rule prose and the token `column_descriptions` in the yaml example. REQ-OOS-05 requires an explicit `include_unreviewed` and a visible mark, and it forbids widening classification or grants. The compiler implements the yaml token and the spec mark. The prose token is refused. That refusal is the check against a silent grant. This review does not treat the prose wording as a product defect to patch, and it does not treat the token as approval.

## Not held

This packet does not show a live Builder call, a PostgreSQL session, an authenticated principal, a grant, or a publication. a315's occupied dataset CAS was not re-opened as a new unit. The column CAS exercised here is the parallel optional flag on the same pack command. `tools/pg-builder` was outside the archive mount.

Proof-1 and proof-2 are kept failures of the oracle harness. They are described in `REPORT.md`. They do not change the proof-3 verdict.
