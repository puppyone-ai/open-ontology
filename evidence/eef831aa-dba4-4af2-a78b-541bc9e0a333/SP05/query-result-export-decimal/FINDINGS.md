# Findings

Scope is the fixed commit `63566b60aabdf29a29bfe55c4866edf3c4081cfd`, the a331 withhold, and the owned exporter. Product edits are only under `tools/query-result-export`. The producer, `oo-core`, and the plan ledger were not edited.

## Verdict

Exporter API: the frozen decimal cell is implemented and the focused tests and Clippy exited 0.

Child E44 / FILE-D04 live chain: **WITHHELD**. There is no compiled `query-pg` stdout from a fixed a327 artifact. Hand-built JSON in the unit tests is not that receipt.

No parent acceptance. No new unit.

## Decimal

Kind `decimal` requires `negative`, `coefficient`, and `scale` and no other field. The coefficient is canonical digits. Scale is an integer in `-8192..=8192`. Negative zero is refused. The CSV field is the display text, including a leading minus, with no apostrophe. `int` is still kind `int`. A column may be decimal beside a different column that is int. Two non-null kinds in one column are still a source failure. Null may mix with decimal.

`result_bytes` counts the display length. `decimal_cells` keeps the coefficient and the scale, so a negative scale whose display is only digits is still recoverable from the sidecar. `source_sha256` is still the exact source bytes. A truncated but consistent decimal row still writes the files with `complete` false.

The a327 literals checked in `frozen_decimal_cells_keep_plain_text_and_the_int_kind` are:

| plain text | negative | coefficient | scale |
| --- | --- | --- | --- |
| `12345678901234.5678` | false | `123456789012345678` | 4 |
| `-12345678901234.5678` | true | `123456789012345678` | 4 |
| `0.0001` | false | `1` | 4 |
| `-0.0001` | true | `1` | 4 |
| `0.0000` | false | `0` | 4 |

Typed text `-1` in that row is apostrophe-prefixed. Typed int `-1` is not.

## U+FEFF

`formula_prone` now skips U+FEFF in the leading run, and no other format character. The gold cell U+FEFF then `=1+1` is written as U+0027, U+FEFF, `=1+1`. The file does not start with a BOM. This is one unit case. It is not a compiled receipt and not a wider character campaign.

## Checks that stayed green

Pre-edit smoke, 15 + 3 tests, exit 0, before any overlay. Post-edit 22 + 3 tests, exit 0. Clerk CSV bytes are unchanged. Existing null cells, partial export, path refusal, and the hand-formula prefixes stayed in the suite. `Cargo.lock` hash `28821b6eef8935f7239cb1527d27f2dbae67dd16` matches the fixed commit. `Cargo.toml` hash `79a762e623bed59137e95027fee6fd67cb2733d3` matches the fixed commit.

## Failed attempts kept

`linux/postedit-failed-1`: tests exited 0, Clippy exited 1 because the private toolchain had no Clippy component. `linux/postedit-failed-2`: tests exited 0, Clippy exited 101 on `format_in_format_args`. The third run installed Clippy with `rustup component add` and exited 0.

## Not claimed

A private PostgreSQL numeric query. A compiled `query-pg` decimal stdout. Parent SP05-W06. xlsx. HTTP authentication. Release.
