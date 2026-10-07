# Findings

Parent SP07 is not accepted by this packet. The pre-overlay smoke failure and five proof failures are preserved.

## Preserved smoke

`linux/smoke/docker-run.exit` is 1. `linux/smoke/phases/toolchain.exit` is 0 (`rustc 1.94.0`, linux arm64). `linux/smoke/clippy-component.txt` is `absent`. The pinned Rust image does not ship Clippy. `linux/smoke-clippy/` then installed `clippy-aarch64-unknown-linux-gnu` into that container and exited 0. Removing the container dropped the toolchain layer, so each later container installs Clippy again. The successful proof log is one line: `info: downloading component clippy`, and `phases/clippy-component.exit` is 0.

## Proof attempts

| Directory | docker-run.exit | Cause |
|---|---|---|
| `linux/proof-attempt-1` | 101 | `src/canonical.rs` formatted a `char` with `:04x`. Rust 1.94 does not implement `LowerHex` for `char`. The writer now uses `u32::from(ch)`, the same call as `tools/pg-builder/src/canonical.rs`. |
| `linux/proof-attempt-2` | 101 | `env!("CARGO_BIN_EXE_selfbuild_rules")` was unset while compiling `tests/cli.rs`. CLI tests resolve `target/debug/selfbuild-rules` from `current_exe`. |
| `linux/proof-attempt-3` | 101 | Hand truth expects 4 enums and 20 experts. Bridge `left_id` and `right_id` were given `n_distinct` `5`, which is low cardinality, so the producer added two enum cards and two enum experts (6 and 22). The fixture now uses `40`, which is outside 1..=32 and is not the unique sentinel. Cardinality stays `many_to_one`. `tests/fixtures/d-truth.json` was not regenerated. |
| `linux/proof-attempt-4` | 101 | Clippy `-D warnings`: `collapsible_if` in `produce.rs` and `score.rs`, `manual_range_contains` on the scaled cardinality bounds. |
| `linux/proof-attempt-5` | 101 | Clippy `-D warnings`: `manual_is_multiple_of` on the CLI flag-pair length check. |
| `linux/proof` | 0 | Tests, Clippy, lock subset, sealed CLI, and forbidden counts passed. |

`linux/proof-attempt-3/test.log` is the api failure: enums 6 versus 4, experts 22 versus 20. The passing log is `linux/proof/test.log`: 4 lib, 12 api, 6 cli, 0 failed.

## Rule notes that the tests lock

- Lexicon runs before primary key and before type. `dh` stays `unknown` / `unknown` / `pending` even with a small distinct count. `bool` is never an enum.
- A negative `n_distinct` is a fraction of `reltuples`. `-1` is unique and is not low cardinality. Shop `label` (`-0.94725` of 8000) and `memo` (`-0.9206333` of 90000) stay unknown and pending. `qty` stays `quantity` and auto-accepted.
- Two naming parents set `conflict` on each candidate and `selected` null. No candidate is dropped.
- A declared foreign key with unequal types stays in the relation list with `type_compatible` false. The column stays `unknown` / `unknown` / `pending`.
- An empty profile `tables` array is zero profile. Card `n_distinct` is ignored, so `demo.item.zt` stays unknown even when the card says `"3"`.
- A cards `schema_hash` that differs from the catalog is `identity.stale` and writes no document.
- Input longer than 1_048_576 bytes is `bound.input` with empty stdout.
- Duplicate keys, including a `\u0061` alias, are `json`.
- `auto_accept` `none` forces every column `pending`.
- Scoring a review whose `order.phone` `pii` was forged to `none` yields `pii_leak_count` 1 and `matched` false. The scorer does not call the producer.
- Comment objects and `untrusted_comment` are not copied. The shop party comment text and its sha256 are absent from the review.

## Lock

`tools/selfbuild-rules/Cargo.lock` is the session-transition lock with the package name `selfbuild-rules`. Eleven registry triples are present in the root lock. No `[features]` section was added. No dependency was updated.
