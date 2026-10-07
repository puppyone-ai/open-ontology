# Findings

Review is limited to `tools/query-result-export` and this evidence directory, plus the producer boundary the exporter reads. The fixed producer is `contracts/gateway/query-pg-cli-v1.md`, `crates/oo-gateway/src/bin/query-pg.rs`, and `docs/10-security.md` section 6 at `e8d297f95b32052ea587fe8ad67b6e8493a00300`. Blobs of the contract, `query-pg.rs`, and `10-security.md` are in `source/blobs/`. No code outside the owned paths was edited for this review.

## Producer fields that exist

`SummaryLine` serializes `record`, `turn`, `status`, `truncated`, `returned_rows`, `result_bytes`, `rollback_confirmed`, `socket_dropped`, `end_audit_ok`, `original_sha256`, `executed_sha256`, `stop`, and `sqlstate`. `OutCell` is tagged `kind` with `null`, `bool`, `int`, and `text`. `null` has no `value`. `int` and `text` values are JSON strings. There is no `query_id`, column name, fingerprint, or HTTP field on stdout. The sidecar sets `citation.in_source` false and `citation.query_id` null. It copies the two hashes. It does not derive an id from `original_sha256`.

`docs/10-security.md` says a CSV or Excel cell that starts with `= + - @` gets a prefix. The implementation id is `apostrophe-prefix-after-leading-whitespace-or-control`. The prefix is one U+0027 and the original text is unchanged. Typed `int` cells are exempt, including a leading minus. The baseline has no xlsx writer. `xlsx` in the sidecar is the string `unsupported`.

## What the proof exercised

The clerk fixture is three rows of `shop.order_doc` in table order `order_id`, `customer_id`, `memo`, `posted`, `amount`, `clerk_note`. Cell-byte sum 98 matches the summary. The hand CSV keeps `-9223372036854775808` unquoted and unprefixed, leaves two nulls as empty fields, and keeps the UTF-8 memo and clerk notes. The container `cmp` against that file exited 0.

The hand formula row checks eleven prefixed text cells (indexes 4 through 14), including space, tab, CR, LF, SOH, U+00A0, and U+3000. Int8 minimum, int8 maximum, `0`, and `-1` stay canonical. Text `'=1` is not prefixed again. `say "hi", next` and `line1\nline2` are quoted. Null is `[0,19]`. Empty text is `[0,18]`. `formula_prefixed` counts cells, not the header. A header `=cmd` becomes `'=cmd` with `formula_prefixed` 0.

Also refused, with no complete flag: `decimal`, `numeric`, `01`, `-0`, `+1`, a duplicate key, a raw CR in the source, 513 cells, 2001 rows, a source of 33554433 bytes, a returned-row mismatch, and a caller width that does not match the row. A symlink output directory and an existing `export.csv` are path errors. The existing file's bytes stay `keep`. Zero rows write the header only, column kind `none`. Status `failed` with matching counts is not complete.

Live second write exited 4 and left the first CSV in place. Live truncated source exited 1, kept the same CSV bytes, and recorded `complete` false.

## Boundaries that held

No root manifest or root lock edit. `Cargo.lock` lists one package and no registry dependency. No `cargo update`. No Python oracle. SHA-256 in the crate is `src/sha256.rs`. The evidence hasher is `tools/sha256.rs`, the same bytes as the a266 copy (`55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e`). Unsafe code is forbidden. The archive never gained the overlay. One private network and one builder. No PostgreSQL container was created for this task.

Host Rust 1.98 rejected a 7-word SHA-256 IV (the missing word is `0x1f83d9ab`), a `u16` surrogate scalar, a `Vec<u8>` passed where `&[u8]` was required, and `CARGO_BIN_EXE_query_result_export` alone. Those fixes are in the overlay that postedit hashed. `package-sha256.txt` is that overlay. There was no second container attempt to preserve. Smoke `console.log` was not overwritten.

The partial live input is the fixture with one `sed` replacement of `"truncated":false`. The expected CSV is still the hand file, compared with `cmp`. The digests in `live-sha256.txt` come from `tools/sha256.rs`.

## Residual gaps

These are untested branches of code that the review read, not observed failures.

- Two non-null kinds in one column return `source` from `set_kind`. No fixture mixes kinds in one column.
- U+FEFF is category Cf. `formula_prone` does not strip it. The SOH and NBSP rows use the same predicate. There is no FEFF fixture row.
- An IO error after `create_new` removes that name and any names already pushed. The tests do not inject a failed `write_all` or `sync_all`.
- Duplicate column names, a NUL in a column name, and an empty column list are request failures in `check_columns` and `parse_request_doc`. The CLI tests cover help, usage, the clerk export, and the partial export.

## Not claimed

Parent acceptance, SP05-W06, AT-FILE-05, AT-SEC-03, xlsx, HTTP authentication, download authorization, retention, and release are outside this result.
