# Findings

## Bounded numeric is grantable, and the six-row query returned decimals

`linux/live-3` is the completed private TLS proof at product SHA `976cb4538f621eb5de157cef32976b029c6d17cf`. `install-column-acl` exited 0. `query-pg` wrote six rows. Each amount cell and each wide cell is `kind` `decimal` with scale 4. Row 6 is `kind` `null` for both numeric columns. The summary status is `succeeded`, `returned_rows` is 6, `stop` is null, and `sqlstate` is null. Stdout contains no SQL text.

The coefficients are the hand literals: `123456789012345678` and `12345678901234567890123456789012345678` for the large values, `1` for `0.0001` and its negative, and `0` for `0.0000`. Signs are on the `negative` field. Zero stays unsigned.

## Allowlist

`c4d9316d28ceefefc7bc21cd8ac61ce949232d07` froze the addendum before the source edit. `e9cba7df4b8d20ea8b3794f9efcc813434d7b1dd` then changed only `policy_catalog.rs` and `policy.rs`.

`SUPPORTED_TYPES` is `bool`, `int2`, `int4`, `int8`, `numeric`, `text`. `grantable_base_type` admits numeric only when `atttypmod >= 0` and the high 16 bits of `atttypmod.wrapping_sub(4)` are a precision in `1..=38`. Precision 0, precision above 38, and typmod `-1` stay refused. Declared scale is not rewritten. `typtype` must still be `b`. Comparison signatures, operators, literals, and aggregates were not widened. `required_operator_count` stays 54. Float, date, varchar, and unknown stay off the list.

`supported_base` uses the same predicate and reads `atttypmod`. Classification, PII, the config seal, row security, and the column ACL compiler are unchanged. The a326 `std::slice::from_ref` clippy edits already in these two files stayed.

`numeric(18,4)` typmod `1179656` and `numeric(38,4)` are inside that bound. That is why install exited 0 on this fixture.

## Absent row filter

`linux/live-2` reached install exit 0 and then `before-analyze` exit 3, stderr exactly `query-pg: request`. Reviewed-policy serializes a missing filter as `"row_filter": null`. `load_request` rejected every JSON null before catalog unknown-rows could return exit 5.

`976cb453` allows a null only when the object key is `row_filter`. Every other null stays `Fail::Request`. The harness still embeds reviewed-policy stdout unchanged. On live-3, `before-analyze` exited 5 with stderr exactly `query-pg: policy\n`. The hidden column exited 6 with stderr exactly `query-pg: guard\n`.

## Admission and import

`reviewed-effective-oos` applied `data_type` `numeric` and semantic `quantity` to `shop.entry.amount` and `shop.entry.wide`. `reviewed-policy` emitted one policy. Money, identifier, and the other non-quantity semantics stay refused in the unit tests. Integer quantity is unchanged.

`import-pg` exit 0 copied the six source rows, including the 38-digit value, the negatives, the scale-4 zeros, and the nulls. They were not inserted into the target by hand.

An unbounded numeric column still has no `data_type`. `column_spec` returns semantic before `compatible`. `wrong_physical_quantity_is_semantic` still passes. That pass is the product result in the contract. `linux/postedit/postedit.exit` remains 2. `linux/postedit/inside.sh` no longer treats that pass as a product failure.

## Infinity and NaN stay out of the fixture

`numeric(18,4)` rejects `'Infinity'::numeric` at insert. PostgreSQL reports numeric field overflow, SQLSTATE 22003. `linux/live-failed-2` records that insert. NaN can be inserted into `numeric(18,4)`, and the approved import refuses it with `full import refused numeric_nan`. `linux/live-failed-3` records that refusal. The target was not patched by hand.

The decoder fail-closes those wires in unit tests: `numeric_nan`, `numeric_infinity`, and `numeric_neg_infinity`. Live-3 did not select those values.

## Mapping-reader bound

Live-3 recorded `ensure_supported_type` as a name-only check. `9183e79082341a1a2bd7d3b1a2ae3e8764735d0e` froze the addendum. `21cc96241a841b47dbbcb116549030e38a643ba0` then selects `atttypmod` and calls `grantable_base_type`.

`linux/live-4` imported nullable `loose numeric` (typmod `-1`) and `over numeric(39,4)` (typmod `2555912`) on the same `shop.entry` as the six decided rows. Before the mapping grant, `provision-loose` and `provision-over` each exited 7. Stderr is exactly `provision-mapping-reader: policy type\n`. The login still lacked `USAGE` on `_oo`, `SELECT` on `_oo.physical_mappings`, and `EXECUTE` on `_oo.lookup_physical_mapping(text, text, bigint, text, text, text, text)`. The reviewed three-column policy then installed and provisioned. The amount stdout matches live-3, including `result_bytes` 162 and both summary hashes.

`supported_base` still adds the `pg_catalog` OID equality check. Install and mapping-reader do not. Comparison signatures still omit `numeric`. Domains and enums stay refused. Negative declared scale with precision `1..=38` stays admitted. `whitelist_tokens_ok` and `load_types` still do not apply precision.

## Earlier false green

`linux/live-failed-1` used `--exact live_numeric_proof`. Cargo exited 0 with 0 tests. Live-3 requires `test a327_live::live_numeric_proof ... ok` and a non-empty `outcome.txt`.

`role-pack-bundle` publishes with a hard link. The bundle file was written on the target volume, where the link succeeds.

The proof lock hash before and after is `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`. Workspace `Cargo.lock` at the time of this report hashes to `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474` and was not used.
