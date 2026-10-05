# Findings

Verdict: **APPROVE_LIMITED** for `9daeaa05e34c154391a757fc78fb47f09c954c6a` only.

The independent Linux run agrees with the contract and with the author tests that already passed in the pre-overlay smoke. No product defect was observed. The two red overlay attempts are harness defects and are preserved.

## Binding that held

Explicit decisions are the only classification input. `auto_accept` is stored because the ontology file requires a plugin name. With `accept_everything` and an empty decision list, all six shop columns stay `classification.absent` and `pii` null. A role pack that names one of them is refused with `classification.unknown_pii`.

Applied decisions set `pii`, `semantic_type`, and `classification.state=confirmed` with `by` and `at` from the reviewer. They do not set `review`. `customer.tenant_id` became `restricted`. `purchase.customer_id` became `personal` / `foreign_key`. `email` was left unclassified.

`pii=secret`, `semantic_type=passport`, and an uppercase `schema_hash` are `decision.invalid`. A repeated dataset and column is `decision.duplicate`. An extra `note` field is `field.unknown`. The sentinel `a228-note-do-not-echo` is not on stderr.

A decision whose schema hash and fact fingerprint are both wrong records `decision.stale_schema` and leaves `pii` null. A wrong fact fingerprint with the sealed schema hash records `decision.stale_fact`. The stale schema case is checked first.

A column named `contact` that is not on the effective draft is `decision.unknown`, with empty stdout. Renaming catalog column `email` to `contact` while submitting the old schema hash makes `review-pg-draft` exit 1 with `hash.schema` and empty stdout.

A structural `reject` of `email` removes that column from the effective datasets. A later decision naming `email` is `decision.unknown`. A role pack that names it is `visibility.unknown_column`. The author unit test still covers a rejected column that remains on the draft and is hidden as `column.rejected`. This merge does not emit that shape. The contract at this commit matches the merge.

Tampering the embedded effective `pii`, and separately replacing wrapper `grants` with `authorized`, is `seal.rejected` with empty stdout.

Curated `pii` is not copied onto the effective column. Curated unit `JPY` is kept when the decision says `money`. Money without a unit is `oos.invalid`. `generated_sha256` and `curated_sha256` stay distinct, and a curated override changes only the curated digest.

`oo_r_pay` allowlists `customer_id`, so the pack contains `purchase_customer_fk` and the text `declared_fk (source-declared, not a grant)`. `oo_r_desk` does not allowlist `customer_id`, so the pack has no foreign key, no `customer_id`, and no `email`. The relationship object on the effective document has `review` null. Endpoint closure is the allowlist, not a grant.

Command provenance records `authentication`, `authorization`, `grants`, `publication`, and `semantic_publication` as `not_claimed`. Role-pack `manifest.authentication` and `manifest.publication` are `not_claimed`. `row_security_reproven` is false. The effective object has no `decisions`, `samples`, `credentials`, or `privileges` members. The owner role string and the comment body are absent from the compiler and pack stdout.

`schema_hash` is `bcb019d92270bf12092a3f47697b25719cc087a1ac4b7c56a10cf6dd00294c4c`, the locked shop catalog digest. The harness does not reimplement the catalog preimage.

## Limits that stay open

This approval does not accept M1-D18, and it does not publish an ontology. Authentication, grants, row-security proof, semantic publication, and gates G1–G13 stay open. Role-pack visibility is the caller-supplied snapshot in the command. It is not a live permission check.

`crates/oo-builder/Cargo.toml` still says the crate does not classify. That file is not part of this review's owned edits. The compiler does classify from explicit decisions. The sentence is a stale description, and it is not a failed behavior on this run.

The catalog hash function is not exported. The independent fixture therefore reuses the locked shop catalog bytes and changes the decisions, settings, reviewer, and roles. A second catalog with a different schema hash was not invented here.

No PostgreSQL container was started. Historical `pg_catalog` integration stays `not_accepted` on the seal. No LLM, identity, or publishing campaign was added. No dependency, manifest, lock, or ledger was edited.

`sqlx-postgres` lint noise did not appear in this Clippy log. Clippy exited 0 with an empty warning list.
