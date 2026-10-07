# Numeric query review findings

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a340`. Observations from `linux/proof` on commit `21cc96241a841b47dbbcb116549030e38a643ba0`, the corrected gold rerun, the captured catalog, and the two preserved attempts. Scope is child E46 / M1-D14-N.

Verdict for this child: **APPROVE_LIMITED**.

## What the passing live run showed

- The amounts query returned six decimal rows. Positive and negative `numeric(18,4)` and `numeric(38,4)` keep the hand coefficient and scale 4. `0.0001` and `-0.0001` are coefficient `1` with scale 4. `0.0000` is coefficient `0`, negative false, scale 4. Row 6 is `kind` null for both amount and wide. The source `psql` text matches those six values, including the empty text for the two nulls.
- Source typmods are the bounded forms 1179656 and 2490376, the unbounded form -1, and precision 39 form 2555912. OID 1700 is the `pg_catalog` numeric base type and the column type oid. typtype is `b` on each source column.
- Unbounded `loose` and precision-39 `over` each exited 7 with stderr `provision-mapping-reader: policy type` before `provision-mapping-reader` exited 0. `outcome.json` records mapping grants absent both before and after those refusals. The physical target still lists typmods -1 and 2555912. Their presence on the table is the column that was refused, which is separate from a mapping grant.
- Hidden-column query-pg exited 6 with stderr `query-pg: guard` and wrote no audit. The same amounts shape before `ANALYZE` exited 5 with stderr `query-pg: policy`, empty stdout, and no audit.
- `import-pg` before the target schema was ready exited 1 with stderr `oo-pipeline: target schema is not ready`. The following `import-pg` exited 0.
- Catalog, captured draft, review draft, role pack, role-pack bundle, reviewed policy, install ACL, the successful mapping-reader provision, and analyze all exited 0. Bundle SHA-256 and pack digest match author live-4. Generation is 27. One policy table. SSL is true.
- original_sha256 matches author live-4. schema_hash matches author live-4. executed_sha256 and config_digest are this cluster's physical mapping. The summary and the audit agree on `f03136a329352cbec8ec06fe74cbac7147b26223b25812b17e94501570819326`.
- `row_filter` on the amounts request is null. The archived `query-pg` source allows `Value::Null` only when the key is `row_filter`, and it returns `Fail::Request` for every other null. The focused test `absent_row_filter_null_is_optional` passed.
- `_oo.applied_pos` numeric typmod 2490372 appears on the import machinery tables. It is outside the reviewed amount and wide columns.

## Shared type checks

`policy_catalog.rs` `grantable_base_type` is the shared numeric admission: the name is numeric, typmod is non-negative, and the precision taken from the high 16 bits of `atttypmod.wrapping_sub(4)` is in 1..=38. Scale is left as stored. `supported_base` adds the loaded `pg_catalog` OID equality and refuses typtype other than `b`.

Install `ensure_grantable_type` and mapping-reader `ensure_supported_type` both call `grantable_base_type` and require kind `b`. Their `pg_type` join is `type_row.oid = attribute.atttypid`, which is how they read typname, typtype, and typmod. The corrected gold oracle treats that join as a read. It fails the review only if those functions contain `type_oid` or `types.get`. They do not. `ensure_supported_type` also does not call `SUPPORTED_TYPES.contains`.

`SUPPORTED_TYPES` is `bool`, `int2`, `int4`, `int8`, `numeric`, `text`. Float names are absent from that line. `fn base_type` in `reviewed_policy.rs` contains neither `float4` nor `float8`. `pg_catalog` still has float4 oid 700 and float8 oid 701. Those oids were not column types of `shop.entry`.

The first gold oracle failed four checks that the source does not fail: a refusal test mentions `float8`, the `atttypid` join contains the substring the old oracle called an OID check, `assert!(!base_type("float4"))` mentions `float4`, and the executed hash was pinned to author live-4. Those four lines stay in `gold-attempt-1.txt`. `gold.txt` records the corrected notes and `gold_match=1`.

## Decoder and nonfinite values

The archived decoder constants include OID 1700, `CANONICAL_MAX_COEFFICIENT_DIGITS`, and the fail-closed names `numeric_nan`, `numeric_infinity`, and `numeric_neg_infinity`. Unit tests `numeric_wire_matches_the_reviewed_vectors` and `numeric_hand_literals_keep_scale_and_reject_specials` passed in the 43-test lib run. The live amounts query selected finite stored values. `outcome.json` records Infinity rejected at insert into numeric(18,4) with sqlstate 22003, and NaN refused during import. This review did not select a nonfinite wire value in the live query.

## Comparisons, literals, aggregates

`COMPARISON_SIGNATURES` has no numeric entry. The catalog source has no `NUMERIC_LITERAL`. `AGGREGATE_STAR_NAME` is `count`. Gold notes `numeric_comparison=absent` and `numeric_literal=absent`. The approved surface is the exact decimal cell for a bounded numeric column on this query path. Numeric comparisons, numeric literals, and numeric aggregates stay unsupported. This review makes no full SQL capability claim.

## Harness facts that stay beside the product result

- `docker-run.exit` 1 is the gold phase. Hostconfig OOMKilled is false. The live chain exits above are the product result.
- Builder `/tmp` is noexec. The gold rerun compiled to `linux/proof/gold-bin`, ran it, and deleted the binary. A later `oo.kind=hash` inventory compile uses `/usr/local/bin` for the same reason.
- The frozen live test writes the role-pack bundle to `/target/a327/a327-role-pack-bundle.json`. Attempt 2 failed there. The passing `inside.sh` creates that directory on the private target volume. That is a harness path, and the product test path stays hardcoded.
- Attempt 1 failed before tests because the archive lacked `tools/gateway-token-store`. The retry added that tree from commit `21cc9624` only.
- The first `pg/up.sh` process exited 2 after `pg_isready` succeeded, because `psql` asked for role `postgres`. The running cluster uses `oo_fixture_admin`. `up.sh` and `capture.sh` on disk pass `-U oo_fixture_admin`. `up.exit` is 0. The container was left in place.
- `lock-hash.txt` is absent because `record` exits when gold returns 1. The readonly archive hash of `Cargo.lock` still matches the commit.

## Limits of this approval

This approval covers the compiled chain above: source import, catalog, captured review, role pack, reviewed policy, install ACL, provision, analyze, and query-pg, on PostgreSQL 17.10 with the pinned Rust 1.94 builder, for nullable numeric(18,4) and numeric(38,4) hand values, with unbounded numeric and numeric(39,4) refused at provision before a mapping grant, and with hidden exit 6 and pre-analyze exit 5.

Parent M1-D14 remains open. M1-I01 and M1-I02 remain open. The a317 100k fixture and the CSV decimal integration remain on their existing evidence. Release remains open. Root Axum dependency work and the a337 HTTP review remain outside this child. Namespace OID equality beyond the `supported_base` check recorded here was not turned into a separate campaign. Domains are refused by the typtype `b` check in source and by the gold note `domains=refused_by_typtype`. This run did not create a domain column.
