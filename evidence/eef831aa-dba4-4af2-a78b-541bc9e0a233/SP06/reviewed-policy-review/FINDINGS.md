# Findings

Independent review of commit `baf70c5c7b6d01183a178f53c77f65da4a7bb497` against `contracts/gateway/reviewed-policy-v1.md` and the producer contract `contracts/builder/reviewed-effective-oos-v1.md`. Author evidence is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a227/SP06/reviewed-policy/`. That author review is the same session as the implementation. This directory is a separate session. Verdict `APPROVE_LIMITED`. M1-D14B stays unaccepted.

## Producer output

`compile_reviewed_policies` was called on stdout from the fixed archive's `reviewed-effective-oos` and on the sealed captured-draft artifact that produced it. The ledger fixture is independent of the author shop fixture. Two administrative bindings, `teller` and `audit`, compile two policy sets from one command, one artifact, and one decision array. `status`, `semantic_type`, and `pii` come from the typed effective column after the decision binds. `int8` arrives as `identifier`, `bool` as `boolean_flag`, `text` email as `email`, and `int4` `account_id` as `foreign_key`. The allowlist carries dataset, column, and `explicit_allow` only. The adapter calls `compile_column_acl_set`. The returned role string is `oo_r_` plus the binding slug. `row_filter` is `None` on every policy.

Role, login, config digest, generation, and the allowlist are mapping input. The command's non-claims stay `not_claimed`. Wrapper `publication` is `unavailable` and wrapper `grants` is `not_authorized`. The reviewer label `nina.records` is a decision field the producer requires. It is not an authentication decision. The compiled policy login is the binding login, and the policy debug text does not carry the reviewer. The adapter source does not call `compile_structure_draft`, does not open a database, and does not call an installer.

## Boundary cases

These refusals were produced by editing the real compiler inputs or the real compiler output, then calling the adapter:

| Case | Result |
|---|---|
| Allow column `ghost` | `decision` / `unknown` |
| Decision schema hash replaced | compiler `decision.stale_schema`, adapter `stale_schema` |
| Decision fact fingerprint replaced | compiler `decision.stale_fact`, adapter `stale_fact` for the whole allowlist that includes it |
| Decision for `open` omitted, `auto_accept` still set | compiler `classification.absent`, adapter `missing` |
| Last hex digit of `sealed_source_sha256` flipped | `seal` |
| Last hex digit of command `provenance.draft_hash` flipped | `seal` |
| Catalog owner byte changed | `seal` |
| `draft.provenance.draft_hash` inserted | `seal` |
| `personal` with `explicit_allow` false, and `none` with `explicit_allow` true | `pii_allow` |
| Semantic type `json` on text, and `float4` | `semantic` |
| Binding project `other` | `identity`, display `reviewed document identity does not match the administrative binding` |

## draft_hash

Gateway contract step 7 says the SHA-256 of the canonical draft bytes equals `wrapper.draft_hash` and `provenance.draft_hash` on the draft. The captured-draft producer writes `draft_hash` on the wrapper. The reviewed-effective producer copies that value onto the command provenance, next to `sealed_source_sha256`, `wrapper_hash`, `generated_sha256`, `curated_sha256`, and `schema_hash`. Structure-draft provenance carries `manifest_hash` and `schema_hash`. It does not carry `draft_hash`.

The ledger artifact confirms that layout. The independent SHA-256 of the embedded canonical draft bytes equals both producer fields. `draft.provenance` has no `draft_hash` key. Flipping the command field is `seal`. Writing the digest onto `draft.provenance` does not repair the artifact; that document is `seal` as well. The adapter in `verify_seals` compares `wrapper.draft_hash` and `command.provenance.draft_hash` with the recomputed draft digest. It follows the producer protocol. Requiring `draft.provenance.draft_hash` would reject the compiler output this review consumed.

The contract sentence remains in the frozen file. This review does not amend it. The sentence is a field-placement discrepancy. It is not a seal hole in the adapter at this commit, and it is not a publication or an authentication claim.

## Still open

Authentication, grants, publication, an audit WAL, live row security, and a non-superuser grantor stay outside this adapter. Physical-type proof on a live server stays on the installer path. M1-D14A, M1-D14B, and SP06-W01 stay open. No dependency was added. `Cargo.lock` and the gateway manifest were not edited. a207 remains the connector-manifest and root-lock writer.
