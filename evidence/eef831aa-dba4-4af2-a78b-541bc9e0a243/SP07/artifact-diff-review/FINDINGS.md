# Findings

Verdict: **APPROVE_LIMITED** for `ontology-artifact-diff` on fixed `701f80aa6f2089a865d2ff99b5aa5c1bcda1de6a`. The decisive run is `linux/overlay-3`: author test 9 passed, independent CLI test 5 passed, `test.exit` 0, `review.exit` 0, `clippy.exit` 0, `docker-run.exit` 0.

The preserved `linux/overlay` and `linux/overlay-2` failures are oracle compile and assertion failures. The product binary's author tests and product Clippy exited 0 on both of those archives. No product change was made.

## Independent CLI proof

The oracle builds a ledger catalog and hashes it with the published `pg-catalog-hash/v1` field order before any compiler call. `captured-pg-draft` accepts that document. Reversing the JSON table array yields the same sealed command. The column comment `ledger-comment-do-not-carry-a243` is absent from the draft. The owner `ledger_owner_a243` is absent from the wrapper.

`ontology-artifact-diff` then sees those files:

- Identical inputs, pretty JSON, and a key-reordered command produce one sealed document. `protocol` is `oo.builder.artifact-diff/v1`. `compiler` is `ontology-artifact-diff/v1`. `auto_accept` is false. `publication` is `unavailable`. `certification` is `none`. `grants` is `not_authorized`. `schema_hash_equal` is true. `facts` is empty. `unchanged` is greater than 0. `before` and `after` both name `ledger_src` and `ledger` and carry the same `schema_hash`. Stdout has no trailing newline and equals the canonical document. `diff_hash` is `oo_core::api::payload_sha256` of that document with `diff_hash` removed.
- Renaming `code` to `reference_code` removes `column/public.account/code` and adds `column/public.account/reference_code` on the physical layer and the generated layer. The unchanged `id` column is stale. The source binding is changed. No classification fact is emitted. `schema_hash_equal` is false.
- Changing `code` from `text` / oid 25 to `varchar` / oid 1043 changes the physical column and the generated column. Physical `type_reason` stays `known_builtin_not_converted`. Flag order is byte-identical.
- Shrinking `entry_account_fk` from two pairs to one keeps the id `foreign_key/public.entry/entry_account_fk` and marks the physical and generated foreign-key facts changed. The command uses the ident stored on the artifact.
- `review-pg-draft` with `code` `not_null` flipped, curated classification `{"state":"confirmed","by":"secret-curator-name-a243"}`, and a decision `action` `accept` bound to the before schema hash and the before structural fingerprint produces: physical column changed (`not_null` false to true); generated column stale; curated classification added at `classification/public.account/code` with `unknown` false, `state` `confirmed`, no `by`, and `value_sha256` of the canonical caller object; decision stale with that `accept` token and the before schema hash; queue item stale with `kind` `stale_decision`, `detail` `stale_schema_hash`, and `publishable` false. No generated classification fact appears. The curated column label `do-not-echo-label-a243` is absent from the diff stdout.

Refusal checks on the same binary: usage exit 2 is the four contract lines; a missing path is `io.open`; a directory and a symlink are `io.regular`; a repeated `draft` key is `json.duplicate_key`; a replaced protocol is `version.unsupported`; a zeroed `draft_hash` is `hash.mismatch` and the stderr does not contain that hash; an extra command key is `field.unknown`. Exit 1 writes an empty stdout and one static line. The line does not contain the path or the sentinel text.

## Author tests on the same archive

`cargo test --locked --offline -p oo-builder --test ontology_artifact_diff` exited 0: 9 passed, 0 failed, 0.53s. That includes the shop preimage lock, identical sealed diff, flag order, rename, type change, foreign-key shrink, stale review with curated classification, and resealed `duplicate.fact`. Those tests call `compile_captured_pg_catalog` directly. This review ran them on the fixed archive and did not treat them as a substitute for the ledger CLI.

## Source bounds checked against that behavior

`class_payload` reports `unknown` false, `value_sha256`, and `state` only when `state` matches the contract token. `by` stays inside the hashed caller JSON and is not a diff field. `project_curated` allowlists `label` on a curated column so the echo can contain it, and it does not copy `label` into the classification fact. The overlay-2 panic showed the label still present on the compiler draft. The overlay-3 diff stdout omitted it. That split matches the contract: the structure-draft echo stores caller JSON, and label text is not a diff field.

`read_file` uses `symlink_metadata`, rejects a non-regular file, and checks the opened fd again. The oracle's symlink and directory cases returned `io.regular`. Document max is 1048576 bytes. Digests go through `oo_core::api::payload_sha256`. The command file begins with `#![forbid(unsafe_code)]`. Clippy `-D warnings` on the binary, the author test, and the review test all exited 0.

## Limits left as written

- A one-sided unknown classification or review is omitted and is not counted. Unknown versus unknown is counted in `unchanged` and omitted from `facts`.
- Queue `detail` is copied. The ledger queue detail observed here is the compiler token `stale_schema_hash`.
- The path is a trusted local file. A symlink present at the `symlink_metadata` check is refused. The command does not claim a jail. A replacement between that check and the open is the documented limit.
- Policy text and unique-key membership are not separate diff identities. A schema-hash move they cause still changes the source binding and marks equal table, column, primary-key, and foreign-key facts stale.
- `type_support.converted` true is `field.invalid`. Empty physical `generated` and `identity` strings mean unset.
- Queue items whose `kind` is not `stale_decision` are shape-checked and are not emitted. An after-side `stale_decision` is `stale`.
- The stdout lock drop ignores a flush error after a successful `write_all`.
- The resealed `duplicate.fact` case ran in the author test on this archive. The independent oracle covers an unsealed `hash.mismatch` and does not rebuild a wrapper seal.
- O-D01, SP07-W05, authentication, and publication stay open. This review does not accept a catalog, grant a privilege, certify a draft, or publish.
