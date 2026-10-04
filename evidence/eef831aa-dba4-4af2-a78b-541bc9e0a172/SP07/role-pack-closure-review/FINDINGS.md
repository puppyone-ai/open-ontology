# Findings — a172 role-pack closure

Verdict: **REQUEST_CHANGES** on `ef34ad7bc6d89376d0b1952dfe49e5efb2a8744d`, tree `9f2f6d353a957cc400c7be01d575599260117fe4`.

The measure is REQ-OOS-05 (`specs/open-ontology-v0.1-spec.md` line 497) and `docs/06-ontology-layer.md` section 7.1 (lines 581–586): per role, remove hidden tables and columns and the relationships, metrics, examples, and terms that reference them. Contract `contracts/builder/role-pack-v1.md` line 46: the compiler does not write a count, a name, or a label of a hidden column, and it does not write the label of a hidden dataset. Line 74 keeps a metric only when its SQL stays inside the visible set.

a168 (`d61f9a4c`) required three closures on `3dadf678`. This review re-ran those shapes on a fresh fixture against the fixed archive. F1 and F2 are closed. The non-ASCII label closure is closed for NFC prose, calendar phrases, and quoted identifiers. It is open for a hidden label written with the supported SQL string escape `''`.

## Required change

Decode a single-quoted SQL string before the hidden-label compare. `''` is one apostrophe, the same rule `parse_string` already uses to accept the statement. After that decode, a hidden label must drop the metric or example. A string that is only an apostrophe in visible prose must stay.

`crates/oo-builder/src/role_pack.rs` at this commit:

- `label_in_text` lines 1378–1384 matches the raw NFC source with `text.contains(label)`.
- `take_ident` lines 1467–1488 decodes a double-quoted identifier, and `""` becomes one quote. A `'` does not open an identifier.
- `all_idents` lines 1538–1551 inserts those identifiers and otherwise bumps one character, so it walks through `'签''记'` without building `签'记`.
- `copies_hidden_label` lines 1334–1337 therefore misses the label.
- `parse_string` lines 1775–1791 accepts `''` as one quote. `verified_sql` lines 1604–1610 returns true. `metric_kept` lines 1083–1133 keeps the metric. `render_metrics` lines 2223–2226 copies `metric.sql` into the pack.

Contract line 54 says a non-ASCII hidden label matches when the NFC text contains that label, and that the same label matches a quoted identifier after `""` is decoded. Line 64 says a single-quoted string is in the supported subset and that `''` is one quote. The parser follows line 64. The closure follows the raw-text half of line 54 and does not decode `''`. The copied SQL is the label.

Fresh proof, deciding run `linux/probe-3/`. Clerk role `oo_r_clerk`, principal `ada`, cannot see column `tick_col`. That column's label is `签'记`. The pack file `linux/probe-3/out/clerk/files/metrics.md` lines 69–81 still contains:

```sql
SELECT '签''记' AS mass_kg FROM dock.parcel
```

under heading `## sq_leak — 单引号标签`. `linux/probe-3/out/outcome.json` records the only closure violations as `` `签''记` in metrics.md `` and `hidden or unsupported object kept: ## sq_leak`.

The same pack keeps the visible string metric:

```sql
SELECT 'it''s visible' AS label_ok FROM dock.parcel
```

under `## string_metric`. A repair that drops every apostrophe string fails that case.

Quoted-identifier doubling is already closed on this commit. Metric `mix_alias` (`SELECT mass_kg AS "签""记" FROM dock.parcel`) is absent. `take_ident` decodes that alias to `签"记`, which equals the hidden label, and the metric is omitted.

## What the fresh fixture showed closed

Clerk table `linux/probe-3/out/clerk/files/tables/dock.parcel.md`:

- `parcel_id` keeps label `包裹号`, type `bigint`, and description `本季到港重量`.
- `mass_kg` keeps the column row. The type cell and the notes cell are empty. The source type was `numeric hold_bin`, the source unit was `HOLD_BIN`, and the source description named `"pier"."berth"` and `é货签`. Those strings are absent.
- `mark_ok` keeps the note `可见标记说明`. Its source label was the hidden NFC label `é货签`, and that label is absent.
- `badge` keeps `Samples are omitted because pii is personal.`

Calendar `linux/probe-3/out/clerk/files/calendar.md` keeps `本季: month_start .. today`, anchors `month_start` and `today`, and `Asia/Shanghai`. It omits anchor `quarter_start`, phrase label `hold_bin`, phrase label `é货签`, and phrase `锚点泄漏` (its start expression is the hidden column `quarter_start`).

Omitted headings in the same outcome file, each `kept: false`: `## mix_quoted`, `## mix_case`, `## mix_ws`, `## mix_alias`, `## mix_nfc`, `## mix_token`, `## mix_path`, `## prose_metric`, `## join_metric`, `## or_metric`, `## comment_metric`, `## semi_metric`, `## star_metric`, `## with_metric`, `## cast_metric`, `## sub_metric`, `## distinct_cols`, `## union_bare`, `## unclosed_metric`, `## unit_leak`, `## ctrl_leak`, `## syn_leak`, `## nfd_leak`, `-- ex-hidden-quote`, `-- ex-nfc`, `-- ex-join`, `-- ex-prose`, `-- ex-double`, `## 引用泊位`, `## 草稿`, `## 泊位术语`.

Kept legal objects: `## kept_mass`, `## cmp_metric`, `## count_metric`, `## string_metric`. Examples file:

```text
-- ex-kept
SELECT sum(mass_kg) AS total FROM dock.parcel
-- ex-kept-quote
SELECT mass_kg AS "净""重" FROM "dock"."parcel"
-- ex-scale
SELECT count(*) AS n FROM dock.scale
```

`kept_mass` SQL in `metrics.md` is `SeLeCt "mass_kg" AS "净""重" FROM "dock" . "parcel"` plus `UNION ALL` plus `SELECT mass_kg FROM dock.scale`. Glossary keeps `在港` / `本季到港重量，只计可见列`. Hidden dataset `pier.berth`, row counts `5151515` and `7777777`, and the tokens `hold_bin`, `暂存格`, `泊位单`, `review.a172`, `class.a172`, and `owner.a172` are absent from the clerk files. The closure violation list would have named them.

Audit principal `cy`, role `oo_r_audit`, sees `dock.scale` only. It keeps `## count_metric`, `本季:`, and `estimated rows: 19` on `tables/dock.scale.md`. Pier principal `bea`, role `oo_r_pier`, sees `pier.berth`. The same test required `tables/pier.berth.md` and `entities/berth.md`, and required `tables/dock.parcel.md` to be absent. Those checks added no violation.

`ada` and `ada2` share pack digest `a71764b8f766a2bd48ab65c44e2e6225b00f5a7bcbe20098391389ec42eb39b5`. Their cache keys differ by `principal=ada` and `principal=ada2`. Audit digest `50f6803cf24510398b4a7e6dbf486899515950b5763438ff86ba3adb5f1ebbcd`. Pier digest `a7c7578efc7214c458c6d79a691ba8b0d2a4d103d81aa37cf75a8d20aa631b01`. The independent `sha2` check matched file hashes, byte lengths, `pack_digest`, `cache_key_sha256`, and `manifest_sha256`. No hash violation was recorded.

Manifest fixed fields on the clerk pack: `authentication`, `authorization`, and `publication` are `not_claimed`; `mount` is `not_performed`; `samples` is `omitted`; `domains`, `file_splitting`, and `search` are `unsupported`; `row_security_reproven` is false; `tokens.status` is `unsupported` and `tokens.estimator` is `none`. The artifact has no `principal`. `domains/` is absent. Refusal cases in the same test (stale policy, bad role, `per_role` false, raw samples, unsupported fields, missing and false RLS proof, proof on a denied dataset, unknown PII, incomplete classification, path traversal, duplicate-key display) added no violation.

Author tests on the unmodified archive, smoke and probe-3: 10 passed, 0 failed. `cargo clippy --locked --offline -p oo-builder --all-targets -- -D warnings` exited 0 on the archive (smoke) and again with the oracle present (probe-3). `role_pack.rs` has no `unsafe` token. `lib.rs` line 10 is `#![forbid(unsafe_code)]`.

## Notes that do not replace the required change

`SELECT DISTINCT mass_kg FROM dock.parcel` is omitted (`## distinct_cols`). `SELECT DISTINCT FROM dock.parcel` is kept as `## distinct_bare` (`metrics.md` lines 35–47). `SQL_KEYWORDS` lines 1562–1564 lists `select`, `from`, `where`, `union`, `all`, `as`, `and`, `is`, `not`, and `null`. `DISTINCT` is parsed as a column name. Contract line 69 says `DISTINCT` is unsupported and the metric is omitted. This statement names no hidden column. It is a subset-truthfulness miss on the degenerate form.

A separate compile names a visible relationship and a visible entity `hold_bin`, which is also a hidden column name. `select` lines 988–1016 includes the relationship from the endpoint allowlist and does not scan the name. `render_relationships` line 2368 writes `## hold_bin`. `render_entity` line 2516 writes `# hold_bin`, and the file is `entities/hold_bin.md`. Probe-3 `out/collision/` shows both. Contract line 73 keeps that relationship when the endpoints are allowlisted and the claim is `declared_fk`. Contract line 46 still forbids writing the hidden column's name. The clerk pack in the main fixture does not contain `hold_bin`. This collision is recorded. It is not the copy-site leak a168 named in `data_type`, `unit`, and calendar phrases.

Parse success is not execution and not a grant. The visibility document is an input. This compiler does not query PostgreSQL and does not re-prove row security.

## Still open

Domains, the tokenizer, the read API, search, filesystem mount, authentication, the policy adapter, and publication gates G1–G13 stay open. This review does not approve SP07-W06. No ledger checkbox was marked. No release was cut. No product file was edited.
