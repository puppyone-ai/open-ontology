# Findings

## What the green run established

The library compiles a real `oos/v1` document through `oo-core` serde and `validate_*`. The focused fixture has two roles in two projects (`alpha` / `oo_r_analyst`, `beta` / `oo_r_support`), hidden columns, a hidden relationship, a hidden metric, hidden examples, an approved glossary term kept for one role and dropped for the other, PII without samples, RLS row counts omitted unless the snapshot carries `global_stats: permitted` and a `proven: true` proof, and a separate artifact per project.

Principal `ada` and `ada2` produced equal artifact bytes and equal `pack_digest`, and different cache keys. Repeated compiles matched. Stale policy version, unknown allowlist column, unknown PII, pending classification, missing or misplaced RLS proof, `per_role: false`, raw samples, unsupported fields, a bad `include_unreviewed` token, a duplicate JSON key, a file budget, an entry budget, and a zero entry limit all refused with the codes in `contracts/builder/role-pack-v1.md`.

`hashes_match_an_independent_oracle` recomputes SHA-256 with the `sha2` dev-dependency and a test-local canonical encoder. It checks each file digest, the manifest hash, `pack_digest`, and `cache_key_sha256`. The library hasher remains `canonical::sha256_hex` (`oo_core::api::payload_sha256`). No production dependency was added.

Path lookup rejects traversal, absolute paths, and empty segments, and normalizes `e\u{0301}` to `é`. The rejected path is not copied into the error.

## Defects fixed before the green run

1. `attempts/001` (`a167test01`, exit 101). `ClassState` does not implement `Copy`, so `record.state` could not move out of a shared classification record. The match now uses `&record.state`. Fetch stayed offline. Clippy did not run past the type error.

2. `attempts/002` (`a167test02`, exit 101). The nine integration tests passed. `cargo clippy -D warnings` on the library reported nine `collapsible_if` sites and one `single_match` on `column.pii`. Those are let-chains and `is_none()` in the committed source. The test target did not reach its own lints until the library compiled cleanly.

`attempts/003` is the green record. `focused/` is the original copy of the first failure and was left in place. `attempts/001/` is a second copy of that same failure.

## Bounds a reviewer should treat as specified

- Byte and entry ceilings are enforced. `readme_tokens` and `table_file_tokens` are labels on the manifest. There is no tokenizer, so a byte length is not a token count, and a file over the byte budget is refused instead of split.
- Dependency closure scans ASCII identifiers. It is not a SQL parser. A hidden column named `id` drops user text that contains that token. Compiler boilerplate is not scanned.
- `containment`, `seen_in_query_log`, and `confidence` are never copied. `name_match` and `type_compatible` appear only when both relationship ends allow global statistics.
- An allowlisted column with unknown PII or a classification other than `confirmed` refuses the pack. `classification.auto_accept` is not applied. Review approval does not add a column to the allowlist.
- RLS statistics require the snapshot's own proof. The compiler sets `row_security_reproven` to false and does not query a database.
- Owner names, `verified_by`, `review.by`, and `classification.by` are omitted. Text containing `secret://`, `postgres://`, `mysql://`, or `password=` refuses the pack.
- `domains/`, search, mount, publication gates G1–G13, authentication, and authorization are stated as unsupported in the manifest.

## Not exercised

No PostgreSQL container. No filesystem mount. No HTTP API. No security or edge matrix beyond the nine tests. No host `cargo`. The pre-edit smoke ran `oo-builder --lib` only, so it did not execute `tests/role_pack.rs`.
