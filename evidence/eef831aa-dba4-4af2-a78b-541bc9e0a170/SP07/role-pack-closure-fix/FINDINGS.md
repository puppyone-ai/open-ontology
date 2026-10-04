# Findings

The measure is REQ-OOS-05 and `docs/06-ontology-layer.md` section 7.1: per role, remove hidden tables and columns and the relationships, metrics, examples, and terms that reference them. Review `d61f9a4c` found three leaks on `3dadf678`. This commit changes the closure so those copy sites follow the same visibility rules, and it states the supported SQL subset in the contract.

The deciding test is `closure_keeps_visible_text_and_omits_hidden_references` in `crates/oo-builder/tests/role_pack.rs`, green on `linux/focused-2`. The nine tests that already existed on `3dadf678` passed on `linux/preedit`, `linux/focused`, and `linux/focused-2`.

## F1. User-text render sites

Review sites were `data_type`, `unit`, and calendar phrase labels. On this commit:

- `render_table` copies `data_type` only when `visible_prose` accepts it (`role_pack.rs` around the `data_type` cell). A failing type is an empty cell. The column row remains.
- `column_notes` copies `unit` only when `visible_prose` accepts it (the `unit {}` note).
- `render_calendar` skips an anchor whose token fails `visible_prose`, and `phrase_visible` requires the phrase label and every anchor expression in that phrase to pass. If none remain, the file says `None.`

The clerk fixture keeps `weight_kg` with `data_type` `numeric queue_note` and `unit` `queue_note`. The green test requires `bigint` on `shipment_id`, the approved description `本月称重记录`, and the column name `weight_kg`, and it requires the token `queue_note` to be absent from the clerk prose. Tally keeps `unit kg`. The clerk, audit, and beta calendars keep the phrase `本月` with anchors `month_start` and `today`. The anchor `quarter_start` is also a hidden column name, and the test requires that token to be absent.

`business_tz` stays a validated token and is not scanned. Generated sentences such as the `oo_dict.enum_values` line are not post-scanned.

## F2. Quoted schema.table

`take_ident` reads a PostgreSQL quoted identifier and turns `""` into one quote. `qualified_parts` allows optional whitespace around `.`. A chain of two or three identifiers is the same dataset rule as before: case-insensitive after decode, system schemas fail closed, `oo_dict` and `oo_entity` are not visibility edges, and an unknown table in `include_schemas` drops the object.

The clerk fixture's `quoted_mix` SQL and the glossary definition that name `"yard"."slip"` are omitted. The green test requires the substrings `yard` and `slip` to be absent from clerk prose, and it requires `tables/yard.slip.md` and `entities/slip.md` to be absent for the clerk. The beta role, whose allowlist is `yard.slip`, still receives those two files.

Legal quoted SQL stays. The kept metric `quoted_visible` is:

```sql
SELECT "weight_kg" AS "净""重" FROM "depot"."shipment"
UNION ALL
SELECT weight_kg FROM depot.tally
```

The green test requires `## quoted_visible`, `净""重`, `"depot"."shipment"`, and `UNION ALL` in `metrics.md`, and the example `ex-quoted-visible` with alias `重量`.

## F3. Non-ASCII labels after NFC

ASCII identifier scans still do not treat a non-ASCII label as an identifier token. A hidden label that is not an ASCII identifier matches when the NFC text contains the NFC label. The hidden label in the fixture is stored NFD (`e` + combining acute + `标记`) and the leaked phrase and description use NFC `é标记`. Both forms are required absent. The column `mark_visible` stays, and its description that contains `é标记` is omitted.

A quoted identifier is also compared after `""` is decoded, so a hidden label written with a doubled quote inside the quotes matches by equality. ASCII identifier-shaped labels still match as whole tokens, case-insensitively, so a short label does not delete a longer visible name by substring. A visible dataset's own label is not treated as hidden.

## Verified SQL

`verified_sql` parses one statement: `SELECT` list `FROM` one relation, optional `WHERE` of `AND` comparisons, optional `UNION ALL` of that shape. Names may be quoted. A function call may be `name(*)` or `name(expr, ...)`. String literals use `''`. Keywords are case-insensitive: `select`, `from`, `where`, `union`, `all`, `as`, `and`, `is`, `not`, `null`.

`metric_kept` and `example_kept` omit the object when `metric.sql`, example `sql`, or `control_total.sql` does not parse. After a successful parse, the copy scan still runs on the raw text, so a hidden name inside a string or an alias is not published. The green test omits `prose_metric` (`weight is just prose`) and `ex-join` (`JOIN` of two visible tables) and keeps `weight_total` and `ex-weight`. Control SQL is scanned and the token `control_sum` is required absent.

The contract section "What is compiled" states this subset, the NFC and quote rules, and that parse success is not execution and not a grant.

## Fixture carried from a168

Project `alpha`, role `oo_r_clerk`, principals `ada` and `ada2`. Policy `pol-alpha@7`, knowledge `know-alpha@3`. Allowlist adds `mark_visible` next to the a168 shipment columns so a leaking description can be dropped while the column remains. Hidden column `queue_note` label `客户备注`. Hidden dataset `yard.slip` label `Slip`, row count `8888887`. Shipment global stats denied, row count `4242427` omitted. Tally permitted, `proof-tally-1`, estimated rows 17, with the sentence that the compiler did not re-prove the claim.

`ada` and `ada2` share artifact bytes and pack digest and differ by principal in the cache key. Audit principal `cy` role `oo_r_audit` sees only `depot.tally`. Beta project `beta` principal `bea` sees only `yard.slip`, and row count `8888887` stays omitted because global stats are denied. The test requires `shipment` absent from audit prose and from beta prose. Audit calendar still contains `本月`. Beta calendar still contains `本月`.

Relationship `declared_link` stays source-declared. `guessed_link` stays omitted. Confidence `0.272727`, containment, and query-log count `515151` stay absent. `handle` still says samples are omitted because pii is personal. `class.clerk` and `review.clerk` stay absent. Manifest authentication, authorization, and publication stay `not_claimed`. Samples stay `omitted`. `row_security_reproven` stays false. Tokens status stays `unsupported`. The artifact has no `principal` field.

Glossary keeps `在册` and `可见术语` with definition `本月发货重量，不含隐藏对象`. It omits `引用隐藏`.

## Preserved failure

`linux/focused/` is the first post-edit binary. Its closure test reached the absence loop and had already required `queue_note`, `客户备注`, `yard`, and `slip` to be absent. It then panicked at `assert_absent` because the needle `ex-quoted` is a prefix of the kept example id `ex-quoted-visible`. Clippy on that binary exited 101 for `needless_character_iteration` at the two `is_ascii` checks. Those two edits are in `ef34ad7b`. `linux/focused/` was not overwritten.

## Still open

Domains, a tokenizer, a read API, search, mount, a database re-proof of row security, and publication gates G1–G13 were not part of this change. The compiler still does not claim them. This file is not an acceptance of the review.
