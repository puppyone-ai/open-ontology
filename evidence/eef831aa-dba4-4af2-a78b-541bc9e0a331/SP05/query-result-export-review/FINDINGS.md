# Findings

Review scope is the fixed commit `15558917e2c5be12b74dd1d64fb542b7d05a03af`, the author packet `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a325/SP05/query-result-export/`, and the frozen E44 line. Product, root, and source were not edited. The oracle lives under this evidence directory and was compiled with `rustc` inside the proof container. It is not a new crate test.

## Verdict

**WITHHELD** for child E44 / FILE-D04.

Separated from the parent: SP05-W06, AT-FILE-05, AT-SEC-03, xlsx, HTTP authentication, download authorization, retention, deletion, the reaper, and release stay open. No parent acceptance.

The author tests and the independent oracle exited 0. That green run is the record of the int/bool/text/null CSV path. It is not acceptance of the frozen child.

## Exact decimal

The plan line requires `exactdecimal` next to types, null, and the truncation sidecar. The fixed baseline's cell kinds are `null`, `bool`, `int`, and `text`. `oid_allowed` is bool and int2/int4/int8 and text. OID 1700 is refused before a row is written. The compiled clerk fixture's amounts are canonical int8 text, including the minimum, the maximum, and `0`. The fixture has no decimal point.

The exporter returns source failure for kind `decimal` and kind `numeric`. The proof observed exit 5, stderr `query-result-export: source`, and no files, on hand-grammar inputs. Those inputs are not compiled receipts.

`tools/query-result-export/CONTRACT.md` says exact decimal means the canonical text of a typed int. The frozen child asked for exact decimal in addition to the integer text the baseline already had. Substituting the int sentence for the decimal clause would shrink the child. This review does not do that.

Smallest fix: wait for a compiled query-pg stdout that contains an exact decimal cell, then copy that text unchanged into the CSV and the sidecar. Do not satisfy the clause by editing the contract definition. Do not invent a decimal row and call it query-pg output. The producer change is outside this owned path and was not made.

## Checks that held

Author `cargo test` at Rust 1.94: 15 tests in `tests/api.rs` and 3 tests in `tests/cli.rs`, exit 0. Clippy `--all-targets -- -D warnings` exit 0. No registry dependency. `cargo update` was not invoked. Fetch was `--locked --offline`, exit 0.

Independent oracle, compiled binary, exit 0 unless noted:

- Hand gold CSV matches the binary output. Source copy matches the fixture. Sidecar hashes match `tools/sha256.rs`.
- Null cells `[[1,2],[1,5]]` are listed. Empty text count is 0. The CSV empty field does not itself prove null.
- Existing output name: exit 4, token `path`, prior bytes kept.
- Consistent truncation: exit 1, token `partial`, `complete` false, `partial` true, `truncated` true, CSV unchanged.
- Text after space, tab, CR, LF, SOH, NBSP, and U+3000 is apostrophe-prefixed. Typed `-1` and the int8 minimum are not. Text `-2+3` is.
- Quotes and an embedded newline follow the quoted-field rule in the committed hand-formula row.

## U+FEFF

Observed class `unprefixed`. The data field begins with UTF-8 `EF BB BF` and then `=1+1`, with no U+0027. The file does not begin with a BOM.

`REQ-SEC-02` and `docs/10-security.md` section 6 name cells that start with `= + - @`, including leading whitespace or a control. U+FEFF is a format character. The implementation documents that format characters are not removed. The observation matches the code and that text.

Smallest fix, if a later freeze requires this one bypass closed: add `ch == '\u{FEFF}'` to the leading skip in `formula_prone`. Do not skip every Cf character in that change. This review did not apply the edit and does not withhold the child for it.

## Not exercised

A column that mixes two non-null kinds, and a failed `sync_all` after `create_new`, are implemented and were not given a separate oracle case. No failure of those paths was observed. They were not expanded into a new campaign.

## Not claimed

Parent acceptance, SP05-W06, AT-FILE-05, AT-SEC-03, xlsx, HTTP authentication, download authorization, retention, and release are outside this result. No new unit count. No rework count.
