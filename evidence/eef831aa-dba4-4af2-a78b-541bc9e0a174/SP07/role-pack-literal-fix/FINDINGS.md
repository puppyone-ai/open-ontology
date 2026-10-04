# Findings

The measure is REQ-OOS-05 and `docs/06-ontology-layer.md` section 7.1: per role, remove hidden tables and columns and the relationships, metrics, examples, and terms that reference them. Review `95289c50` on `ef34ad7b` closed a168 F1 and F2, and closed the non-ASCII label closure for NFC prose, calendar phrases, and quoted identifiers. It left one required change and two notes. This commit closes those three on the same compiler. It does not add a second SQL parser.

The deciding test is `escaped_sql_label_distinct_and_name_closure` in `crates/oo-builder/tests/role_pack.rs`, green on `linux/commit`. The ten tests that already existed on `ef34ad7b` passed on `linux/preedit`, `linux/focused`, `linux/focused-2`, and `linux/commit`.

## Required change: SQL string `''`

On `ef34ad7b`, `label_in_text` compared the raw NFC source. `parse_string` already accepts `''` as one apostrophe, so `SELECT '签''记' AS mass_kg FROM dock.parcel` parsed and `render_metrics` copied it. The hidden label `签'记` was published.

On this commit, `scan_texts` (`role_pack.rs` 1291–1295) NFC-normalizes the source, then `decode_supported_sql_strings` (1400–1419). A `'` opens a string only outside a double-quoted identifier and only when the previous character is not an identifier character. `decode_one_sql_string` (1444–1462) turns `''` into one apostrophe and returns `Err` when the quote never closes. `copy_quoted_ident` (1422–1441) copies a `"..."` span, including `""`, and fails closed when that span is unclosed. The decoded text is NFC-normalized again. `copies_hidden_label` (1349–1352) searches that decoded text. `all_idents` and `scan_one` still walk the original NFC text.

Decode does not rewrite the file that is copied. A legal string stays in source spelling.

The fixture metric `sq_leak` is `SELECT '签''记' AS mass_kg FROM depot.parcel`. The green test requires `签'记`, `签''记`, `sq_leak`, and `单引号标签` absent. `string_metric` is `SELECT 'it''s visible' AS label_ok FROM depot.parcel` with control `SELECT sum(mass_kg) AS control_sum FROM depot.parcel`. The test requires `## string_metric` and that exact SQL spelling, and it requires `control_sum` absent. The glossary definition `it's a visible note` stays, because the apostrophe follows an identifier character and is not opened as a string.

`open_quote` has legal SQL `SELECT mass_kg FROM depot.parcel` and description `see '签''记`. The quote never closes, so the scan fails closed. The test requires `open_quote` and `see '签` absent. `control_leak` uses the legal display SQL and puts the leak SQL only in `control_total.sql`. The test requires `control_leak` and `对账泄漏` absent. Control text is scanned. It is not copied.

The contract states this rule at `contracts/builder/role-pack-v1.md` line 56. Dollar quotes, `E''` strings, and comments stay outside the subset.

## Note: unquoted DISTINCT

`SQL_KEYWORDS` (`role_pack.rs` 1649–1651) now includes `distinct`. `parse_name` (1832–1839) rejects a keyword before it accepts an identifier. `parse_select` (1714–1724) parses the select list through `parse_item` / `parse_expr` / `parse_name`, so both of these fail `verified_sql` and the metric is omitted:

- `SELECT DISTINCT mass_kg FROM depot.parcel` (`distinct_cols`)
- `SELECT DISTINCT FROM depot.parcel` (`distinct_bare`)

The green test requires `distinct_bare`, `distinct_cols`, and `DISTINCT` absent. The contract line 71 says unquoted `DISTINCT` is reserved and that the degenerate form is not a column named `distinct`. The fixture does not include a quoted `"DISTINCT"` identifier. `keyword_here` matches the unquoted keyword text; a quoted identifier is still `take_ident`.

## Note: relationship and entity names

`select` (1014–1016) includes a relationship only when the endpoints are visible and `visible_prose` accepts `relationship.name`. `entity_fully_visible` (1227–1230) returns false unless `visible_prose` accepts the entity name and the path `entities/<name>.md`. `visible_prose` (2770–2774) calls `scan_texts` with `require_dataset` false, so a legal name that does not name a dataset can stay.

The fixture has two objects named `hold_bin`, which is also a hidden column (`tick_col` label `签'记`, `hold_bin` label `暂存格`):

- relationship `hold_bin`, provenance `declared_fk`, endpoints `depot.parcel.mass_kg` to `depot.scale.mass_kg`, no review field
- entity `hold_bin`, primary dataset `depot.parcel`, key `parcel_id`, display `mass_kg`

Both endpoints are allowlisted. The name still fails the closure, so neither object is written. Example `ex-rel-hidden` names `rel:hold_bin` and otherwise uses visible SQL `SELECT mass_kg AS rel_hidden_alias FROM depot.parcel`. It is omitted because the relationship was not selected. The test requires `hold_bin`, `暂存格`, `tick_col`, `ex-rel-hidden`, `rel_hidden_alias`, `hidden relationship question`, and `declared_fk` absent, and it requires `entities/hold_bin.md` to be `oo.builder.role-pack/v1/path.absent`.

The legal counterparts stay: `## parcel_scale`, example `ex-rel-kept`, alias `rel_kept_alias`, and entity file `# parcel`. The contract rows for Relationship and Entity, and the paragraph after the `declared_fk` sentence (line 82), state that endpoint allowlisting does not publish a hidden name and that an example `rel:` of an omitted relationship is omitted with it.

Entity synonyms and descriptions are not scanned. They are not rendered by this compiler. This change does not add that scan.

## Legal controls that stay

Project `alpha`, ontology `ont-a174`, role `oo_r_clerk`, principals `ada` and `ada2`. Policy `pol-alpha@7`, knowledge `know-alpha@3`. Datasets `depot.parcel` (RLS, global stats denied, row count `515151`, columns `parcel_id`, `mass_kg`, `badge` with pii `personal`, hidden `tick_col` and `hold_bin`) and `depot.scale` (RLS, global stats permitted, proof `proof-scale-1`, estimated rows 19, columns `scale_id` and `mass_kg`).

`ada` and `ada2` share artifact bytes and pack digest. The cache keys differ, and the `ada2` key contains `principal=ada2`. The artifact has no `principal` field. `assert_rust_sha256` accepts both packs.

`tables/depot.parcel.md` contains the sample sentence `Samples are omitted because pii is personal.` and `Estimated row count is omitted.`, and it contains `did not re-prove`. The token `515151` is absent. `tables/depot.scale.md` contains `estimated rows: 19`, `proof-scale-1`, and `did not re-prove`. `kept_mass` stays. Owner `owner.a174`, review `review.a174`, and class `class.a174` are absent.

The manifest checks in the test are `authentication`, `authorization`, and `publication` = `not_claimed`; `mount` = `not_performed`; `samples` = `omitted`; `domains`, `file_splitting`, and `search` = `unsupported`; `row_security_reproven` = false; tokens `status` = `unsupported` and `estimator` = `none`; no `principal` field.

## Preserved failure

`linux/focused/` is the first post-edit binary. Its new test had already required the hidden label, both `DISTINCT` metrics, `hold_bin`, and the hidden example to be absent. It then panicked at `crates/oo-builder/tests/role_pack.rs:467` because the needle `source-declared` occurs in the relationships file sentence "approved, source-declared, or explicitly include_unreviewed." Clippy on that binary exited 0. The assertion in `eff1959b` uses `declared_fk`, which the boilerplate does not contain. `linux/focused/snapshot/` keeps the three files from that run. That directory was not overwritten.

## Still open

Domains, a tokenizer, a read API, search, mount, authentication, a policy adapter, a database re-proof of row security, and publication gates G1–G13 were not part of this change. The compiler still does not claim them. This file is not an acceptance of review `95289c50`.
