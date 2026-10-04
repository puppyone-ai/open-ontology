# Findings

Verdict: **REQUEST_CHANGES**. Deciding artifacts are `linux/probe-3/out/summary/outcome.json`, the dumped pack files under `linux/probe-3/out/{clerk,audit,beta,ada2}/files/`, and `linux/probe-3/out/python-oracle.json`. Probe 1 and probe 2 are harness failures and are not product proof.

The measure is REQ-OOS-05 (`specs/open-ontology-v0.1-spec.md` line 497) and `docs/06-ontology-layer.md` section 7.1 (lines 581–586): per role, remove hidden tables and columns and the relationships, metrics, examples, and terms that reference them. The author record at `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a167/SP07/role-pack-compiler/FINDINGS.md` line 24 says the ASCII identifier scan is a bound reviewers should treat as specified. That disclosure does not shrink REQ-OOS-05. The author's nine tests still pass on this commit. Their fixture does not include a quoted hidden dataset, a non-ASCII hidden label, or a hidden column name placed in `data_type`, `unit`, or a calendar phrase.

## F1. Hidden ASCII column name is copied without a scan

`contracts/builder/role-pack-v1.md` line 46: "The compiler does not write a count or a name of a hidden column." `crates/oo-builder/src/role_pack.rs` lines 12–13 say user-authored labels and prose that contain a hidden column identifier are omitted.

Three render sites copy text with no `visible_prose` call:

- Line 1661 copies `column.data_type` into the type cell.
- Lines 1706–1708 format `column.unit` as `unit {unit}` inside the notes cell.
- Lines 1843–1845 copy every calendar phrase label, then the anchor names.

`render_calendar` (line 1823) runs for every pack. It does not take the visibility allowlist.

Fresh proof, clerk `tables/depot.shipment.md` line 13, column `weight_kg`, which is allowlisted. `data_type` was `numeric queue_note` and `unit` was `queue_note`. `queue_note` is not allowlisted:

```text
| `weight_kg` |  | numeric queue_note |  | none | unit queue_note 称重后对照 "yard"."slip" 的客户备注 |
```

`calendar.md` lines 14–16 are the same bytes in the clerk pack, the audit pack, and the beta pack:

```text
- queue_note: month_start .. today
- 客户备注: month_start .. today
- 本月: month_start .. today
```

Audit's allowlist is `depot.tally` columns `tally_id` and `weight_kg`. Beta's allowlist is `yard.slip` column `slip_id`. Both packs still publish `queue_note`.

The ASCII compare itself drops scanned prose. `note_ascii` description `legacy queue_note alias` and `note_quoted` description `see "queue_note" token` have empty notes in the same table. Metric `ascii_hidden` and examples `ex-ascii` and `ex-token` are absent. `all_idents` at lines 1362–1378 and the compare at lines 1277–1282 are what the author described. F1 is the copy around that function.

## F2. A quoted hidden dataset stays in verified SQL, metrics, and glossary

`qualified_parts` at lines 1330–1359 recognizes `schema.table` only as unquoted ASCII identifiers separated by `.`. The loop at lines 1342–1350 continues only when the next byte is `.` and the byte after that starts an identifier. A quote breaks the chain, so `"yard"."slip"` produces no dataset pair. `all_idents` still extracts `yard` and `slip`. Those tokens are not the hidden column name `queue_note`, so `scan_texts` returns `Ok` when the text also names visible unquoted `depot.shipment`.

That is how these objects were kept:

- `example_kept` lines 1188–1199 scans id, SQL, and question. `ex-quoted` uses `dataset:depot.shipment`. Clerk `examples/verified.sql` lines 1–6 contain `SELECT weight_kg AS "客户备注" FROM depot.shipment` and `SELECT weight_kg FROM "yard"."slip"`.
- `metric_kept` lines 1114–1117 scans SQL, label, and description. Clerk `metrics.md` lines 5–19 keep `quoted_mix` with the same `UNION ALL` and the fenced sentence that contains `客户备注`.
- `term_kept` lines 1142–1145 forces the mapped dataset. Term `引用隐藏` maps to visible `depot.shipment`. Clerk `glossary.md` lines 11–15 keep definition `see "yard"."slip" and 客户备注`.
- `description_text` lines 1740–1747 runs `visible_prose` and then keeps the approved description. The weight description is `称重后对照 "yard"."slip" 的客户备注`, so the notes cell in F1 also carries the quoted dataset.

Clerk has no `tables/yard.slip.md` and no `entities/slip.md`. The hidden dataset is omitted as its own file and named inside the visible files.

REQ-OOS-05 and `docs/06-ontology-layer.md` line 583 require those references to be removed from metrics and examples. Line 586 removes a term whose `maps_to` points at an invisible object. This term maps to a visible dataset and its definition still names a hidden table. Contract lines 53–55 say metric SQL, glossary text, and example SQL stay inside the visible set. Contract line 48 calls qualified `schema.table` a dataset reference. The quoted form is that reference.

## F3. The hidden column's non-ASCII label is not an identifier to the scan

`is_ident_start` lines 1381–1383 accepts an ASCII letter or `_`. `is_ident_cont` lines 1385–1387 accepts an ASCII letter, digit, or `_`. `all_idents` lines 1374–1376 advances one byte on every other byte. The UTF-8 bytes of `客户备注` never form a token, and they do not equal the column name `queue_note`.

The fixture sets that string as the label of hidden column `queue_note` and as a calendar phrase. Probe 3 found it in:

- clerk `examples/verified.sql` line 4, as a quoted alias
- clerk `metrics.md` lines 13 and 17
- clerk `glossary.md` line 13
- clerk `tables/depot.shipment.md` line 13, inside the approved description
- `calendar.md` line 15 for clerk, audit, and beta

Column name `客户` on an otherwise valid column is `schema.rejected` before visibility (`non_ascii_column_name_is_rejected_before_visibility` passed). The leak is the label and the prose, which the OOS text types allow.

REQ-OOS-05 trims hidden columns and the examples and terms that depend on them. Publishing the hidden column's own label in a verified query, a certified metric, an approved glossary definition, and every role's calendar is that leak. The ASCII bound in the author findings does not cover it.

## Held on this commit

These passed on probe 3 and are not change requests.

Cache keys bind project, principal, role, policy, knowledge, ontology, catalog, compiler, and pack digest. `ada` and `ada2` share artifact bytes and pack digest `1005498c6345066a5f88fa1302185d02d2c8b3582831d7f7b1852a9c8be9f90f` and differ in the principal field. Audit digest `367aa80499d60d827df5aad30ad548560ec1a532d93ba0612182367508416f1d`. Beta digest `66b3300e79fcbe09d2f074c062a9e704a76c0e89ae3aeecd8eaac5c4a6c99a29` for the same role name `oo_r_clerk`. The Python oracle matched pack digest, manifest SHA-256, and cache-key SHA-256 on all four dumps.

The manifest on the clerk pack matches the contract's fixed statements: authentication, authorization, and publication `not_claimed`; mount `not_performed`; samples `omitted`; domains, file splitting, and search `unsupported`; `row_security_reproven` false; tokens `unsupported` / estimator `none`; budgets 2000 and 3000 stored as labels. No `domains/` file. No principal field in the artifact or the manifest. README says the pack does not authenticate and does not grant.

RLS shipment omits row count 4242427. Tally emits `estimated rows: 17` with proof ref `proof-tally-1` and the sentence that the compiler did not re-prove the claim. A false proof, a missing proof, and a proof on a non-RLS dataset refuse with `evidence.unproven`. Relationship evidence sentinels are absent. `declared_link` is labeled source-declared, not a grant.

Samples and `most_common_vals` are refused or omitted. Allowlisted unknown PII and a non-confirmed classification refuse. Stale policy, a bad role slug, entry and file budgets, and a zero entry ceiling refuse with the contract codes. Path errors use path `/` and do not echo the rejected text. `handle` is marked personal and the note says samples are omitted. Reviewer and classifier ids are absent.

Domains, tokenizer, read API, search, mount, a database re-proof of RLS, and publication gates G1–G13 were not claimed by this compiler and are not approved here.

## Required change

Close F1, F2, and F3 before this pack compiler can be approved. Hidden column names must not be copied from `data_type`, `unit`, or calendar phrase labels. A quoted `schema.table` that names a hidden dataset must drop the metric, example, glossary term, or prose that contains it. A hidden column's non-ASCII label in descriptions, verified SQL, metrics, glossary text, and calendar phrases must be omitted for roles that cannot see that column. The author's nine tests do not detect these cases. Keep the refusal, sample, RLS-proof, and cache behavior recorded above. This review did not patch the compiler.
