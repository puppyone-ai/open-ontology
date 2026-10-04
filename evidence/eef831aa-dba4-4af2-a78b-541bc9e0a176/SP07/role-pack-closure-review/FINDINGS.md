# Findings — a176 role-pack closure

Verdict: **APPROVE LIMITED** the bounded role-pack compiler library slice on `eff1959bf028a65b20b7fca855d79c23e4b188fd`, tree `2f08d4ecef7ad2f3b617e8a04afcb45f0cf0186f`.

No product line is required to change for the findings this review was asked to close. The deciding artifacts are `linux/probe-2/out/outcome.json` and the pack files under `linux/probe-2/out/{clerk,ada2,audit,pier}/files/`. `linux/probe/` is a preserved harness miss. It is not a second product result.

The measure is REQ-OOS-05 (`specs/open-ontology-v0.1-spec.md` line 497) and `docs/06-ontology-layer.md` section 7.1 (lines 579–591): per role, remove hidden tables and columns and the relationships, metrics, examples, and terms that reference them. Contract `contracts/builder/role-pack-v1.md` on this commit:

- line 46: the compiler does not write a count, a name, or a label of a hidden column, and it does not write the label of a hidden dataset
- lines 50–56: NFC, quoted identifiers, `''` decoded before the label compare, fail closed, copies not rewritten, `'it''s visible'` stays, prose `it's` is not a string
- line 58: the same closure at every copy site, including relationship names, entity names, and `entities/<name>.md`
- lines 60–71: supported SQL subset; unquoted `DISTINCT` is reserved, including `SELECT DISTINCT FROM relation`
- lines 73–83: a relationship or entity whose name fails closure is omitted; an allowlisted endpoint does not publish that name; an example whose `rel:` names an omitted relationship is omitted with it

a172 (`95289c50`) on `ef34ad7b` required the `''` decode. It also recorded two notes: degenerate `SELECT DISTINCT FROM` kept as a column named `distinct`, and a relationship plus entity named like a hidden column. a168 (`d61f9a4c`) F1, F2, and F3 are the regressions this review re-ran on a fresh fixture. All of those are closed on `eff1959b`.

## SQL string `''`

`scan_texts` (`crates/oo-builder/src/role_pack.rs` lines 1282–1337) NFC-normalizes the source, calls `decode_supported_sql_strings` (lines 1400–1419), NFC-normalizes the decode, and compares labels on that decode. `decode_one_sql_string` (lines 1444–1462) turns `''` into one apostrophe. An unclosed string returns `Scan::Affected`. `copies_hidden_label` (lines 1349–1352) then matches the decoded text. `render_metrics` (lines 2310–2312) copies `metric.sql` as written when the metric is kept. The decode does not rewrite the copy.

Fresh clerk pack `linux/probe-2/out/clerk/files/metrics.md` keeps the legal string at lines 67–79:

```sql
SELECT 'it''s visible' AS label_ok FROM wharf.crate
```

The same spelling is `examples/verified.sql` line 14. Glossary line 13 keeps the prose `it's a visible note`. Headings `sq_leak`, `control_leak`, and `open_quote` are absent. A search of the clerk files found no `舱'记`. `control_leak` displayed the legal string and put `SELECT '舱''记' AS mass_kg FROM wharf.crate` in `control_total.sql`. The heading is absent and the legal string remains through `string_metric`, so the control was scanned and was not copied.

## DISTINCT subset

`SQL_KEYWORDS` (lines 1649–1651) includes `distinct`. `parse_name` (lines 1832–1839) rejects a keyword. `verified_sql` (lines 1691–1697) must consume the whole string. Unquoted `SELECT DISTINCT mass_kg FROM wharf.crate`, `SELECT DISTINCT FROM wharf.crate`, and `select distinct mass_kg from wharf.crate` are omitted. Quoted `DISTINCT` is an identifier. Clerk `metrics.md` lines 22–34 keep:

```sql
SELECT mass_kg AS "DISTINCT" FROM wharf.crate
```

Headings `列去重`, `退化去重`, and `小写去重` are absent. That is subset truthfulness. The compiler did not execute the statement.

## Relationship and entity names

`select` (lines 1014–1016) keeps a relationship only when the endpoints are visible and `visible_prose` accepts the name. `entity_fully_visible` (lines 1227–1230) requires the same on the entity name and on `entities/<name>.md`. `example_kept` (lines 1189–1191) drops an example whose `rel:` is not in the kept set. `render_relationships` (lines 2471–2472) writes `declared_fk` only for a rendered declared-fk relationship.

The fresh fixture names a relationship and an entity `vault_bin`, with visible endpoints and provenance `declared_fk`. Clerk `relationships.md` contains only `## crate_scale` (line 5), provenance `human` (line 10). `tables/wharf.crate.md` line 20 lists `crate_scale`. `entities/crate.md` is the only entity file. `entities/vault_bin.md` is absent. `examples/verified.sql` keeps `ex-kept`, `ex-quote`, and `ex-string`. `ex-rel-hidden` and `rel_hidden_alias` are absent. The token `declared_fk` is absent from the clerk files. The relationships introduction still contains the words `source-declared` and `include_unreviewed`. Those words are the boilerplate at the renderer, and this review does not treat them as a leak.

## F1, F2, F3 on the fresh fixture

F1. `tables/wharf.crate.md` line 13 keeps the allowlisted row `| mass_kg | 净重 |  | money | none |`. The source type was `numeric vault_bin` and the source unit was `VAULT_BIN`. The type cell is empty and those tokens are absent. `data_type` is filtered at lines 2153–2158 and `unit` at lines 2204–2207. Calendar `calendar.md` keeps phrase `本季: month_start .. today` (line 14) and anchors `month_start` and `today` (lines 9–10). Anchor `quarter_start` and the hidden phrases are absent. Phrase filtering is `phrase_visible` (lines 2394–2408) and the anchor loop (lines 2346–2354).

F2. Clerk files contain no `pier`, `berth`, or `泊位单`. The metric and glossary that quoted `"pier"."berth"`, and example `ex-hidden-quote`, are absent. Visible quoted SQL stays, unre-written: `metrics.md` line 49 is `SELECT mass_kg AS "净""重" FROM "wharf" . "crate"`, and `examples/verified.sql` line 9 is `SELECT mass_kg AS "净重" FROM "wharf"."crate"`. Pier role `oo_r_pier` still publishes `tables/pier.berth.md` with heading `# pier.berth — 泊位单` and `entities/berth.md`.

F3. NFC label `é舷签` and NFD label `é舷签` are absent from the clerk files. `mark_ok` (table line 15) has an empty label and keeps the note `可见标记说明`. The quoted-identifier leak `"舱""记"` is absent. The doubled-quote form of the visible alias `净""重` remains.

## What stayed

Clerk `metrics.md` also keeps `kept_mass` (line 19, with the unconfirmed-code warning at line 10), `scale_count` (line 64), and `where_metric` (line 94). Glossary keeps `净重口径` with the non-ASCII prose and `在港`. `crate_id` keeps label `箱号`, type `bigint`, and note `本季到港重量` (table line 12). `badge` keeps `Samples are omitted because pii is personal.` (line 14). The pack was not emptied.

Row counts and proofs. Crate table line 7 says global statistics are omitted and the compiler did not re-prove row security. Line 8 says the estimated row count is omitted. Source row count `616161` is absent. Scale table line 7 names proof `proof-scale-a176` and says the compiler did not re-prove that claim. Line 8 says `estimated rows: 23`. Enum `wharf.crate.status.md` says coverage and evidence row counts are omitted. `0.414141` and `191919` are absent. `most_common_vals`, containment `0.818181`, seen-in-query-log `434343`, `owner.a176`, `review.a176`, and `class.a176` are absent. The sentence `Cross-row samples and most_common values are omitted.` is present. That sentence is the renderer’s row-security disclosure.

Audit role `oo_r_audit` sees `wharf.scale` only. Its metrics file contains `scale_count` and not `wharf.crate`. Its only table file is `tables/wharf.scale.md`, which keeps the visible column `mass_kg` and `estimated rows: 23`. A search of the audit files found no `wharf.crate`, `vault_bin`, `616161`, or `泊位单`. Pier files contain `tables/pier.berth.md` and `entities/berth.md`. A search found no `wharf.crate`, `vault_bin`, `616161`, `mass_kg`, or `crate_id`.

## Hash, cache, refusal, manifest

`linux/probe-2/out/outcome.json` records empty `closure_violations`, `kept_misses`, `hash_violations`, and `refusal_mismatches`. Clerk, ada, and ada2 share digest `bb952bc8d8144473889f91525521cf186ea3fef164b1e5688dcba5aba8ab7831`. Audit digest `0432c338eb9792df81d86aea1a86538cd4e589a7e202701d14dfc6d509a6c9fb`. Pier digest `2912af9924aed57f85c7f9e75de427d1ce19470775c80533d7a20b32fe8d7a62`. The ada cache key binds `principal=ada`. The ada2 cache key binds `principal=ada2` and the same pack digest. The independent `sha2` check matched file hashes, byte lengths, `pack_digest`, `cache_key_sha256`, and `manifest_sha256`.

Clerk `manifest.json` is one canonical line. `authentication`, `authorization`, and `publication` are `not_claimed`. `mount` is `not_performed`. `samples` is `omitted`. `domains`, `file_splitting`, and `search` are `unsupported`. `row_security_reproven` is false. `tokens.estimator` is `none` and `tokens.status` is `unsupported`. `role` is `oo_r_clerk`. The manifest has no principal. The file list is `INDEX.md`, `README.md`, `calendar.md`, `entities/crate.md`, `enums/wharf.crate.status.md`, `examples/verified.sql`, `glossary.md`, `metrics.md`, `relationships.md`, `tables/wharf.crate.md`, and `tables/wharf.scale.md`. `domains/wharf.md`, `entities/vault_bin.md`, `tables/pier.berth.md`, and `entities/berth.md` are absent from this role. Manifest SHA-256 is `9d8af51bb5a9951cff5d58969599832fa6ba3bf86a011fab697aa212e9a69f0b`.

Refusal cases in the same test added no mismatch, including `path.invalid` at `/` for `../vault_bin` with the rejected text absent from the display.

Author tests on the unmodified archive and again beside the oracle: 11 passed, 0 failed. `cargo clippy --locked --offline -p oo-builder --all-targets -- -D warnings` exited 0 both times. `role_pack.rs` has no `unsafe` token. `lib.rs` line 10 is `#![forbid(unsafe_code)]`.

## Harness miss, preserved

`linux/probe/oracle.stderr` panics at the oracle assert with `violations=["present `most_common`"]` and empty misses, hash, and refusal lists. Clerk `tables/wharf.crate.md` line 7 and `tables/wharf.scale.md` line 7 contain `Cross-row samples and most_common values are omitted.` The probe-1 outcome already had an empty closure list before that assert, and its clerk digest matches probe-2. The product bytes did not change between the two runs. Probe-2 forbids `most_common_vals` and exits 0.

## Still open

Domains, the tokenizer, the read API, search, filesystem mount, authentication, the policy adapter, and publication gates G1–G13 stay open. This review does not approve SP07-W06. No ledger checkbox was marked. No release was cut. No product file was edited.

A visibility snapshot is an input. It is not authentication and it is not a grant. A successful parse is not SQL execution. This compiler does not query PostgreSQL and does not re-prove row security.
