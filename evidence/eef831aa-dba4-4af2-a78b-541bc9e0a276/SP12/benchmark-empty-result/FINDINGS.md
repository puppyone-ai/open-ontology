# Findings

Session `eef831aa-dba4-4af2-a78b-541bc9e0a276`. Read with `REPORT.md`. This session wrote the seam and the logs below. It is not an independent review.

## a267 not_found mismatch

a267 `FINDINGS.md` records that a declared `not_found` question compiles with an empty column list and gold `kind` `not_found`, while the fixed query-result adapter emits `kind` `rows` with an empty row list. sql-runner then records mismatch. The a267 proof kept both trials in the denominator and expected exit 1. That evidence tree was not rewritten.

This session reproduced that mismatch on fixed archive `8a6395aef5b1d3b16d1b3d72833d2df2ebaa536e` before the overlay. `linux/smoke/red/adapter.stdout` is the empty table. `linux/smoke/red/comparator.stdout` is verdict `mismatch`, path `kind`, actual `rows`, expected `not_found`. The red report is 5 questions, 10 trials, 6 passes, 2 failures, 2 reannotations, `passed` false. `red-runner.exit` is 1.

After the opt-in, `linux/post/green/` is the same 5 questions and 10 trials. The two `pub-absent` trials are pass / match. Reannotation stays 2. `passed` stays false. `green-runner.exit` is 1. The absent adapter stdout is `{"kind":"not_found","completion":"complete","warnings":[]}`. The absent binding contains `not_found_if_complete_empty`. The table binding does not contain `empty_result`. Gold `kind` `not_found` is unchanged. The table cell coefficient remains `120`.

## What the seam does

`empty_result` is optional. The default, including an omitted field and the explicit word `rows`, preserves the current empty-table bytes, including truncated status semantics: `truncated` does not change `completion`. The adapter emits typed `not_found` only under the explicit opt-in and only for a complete clean zero-row result with `max_rows` greater than 0. The conditions are listed in `REPORT.md` and asserted by `opt_in_emits_not_found_only_for_a_clean_zero_row_result`. A non-empty result stays the actual rows, so a comparator that expected `not_found` mismatches. Invalid opt-in values refuse at `/0/empty_result`.

sql-runner accepts the field on the binding and preserves the same string when it rewrites the header. `binding_preserves_empty_result_and_ignores_gold` and `binding_rejects_a_bad_empty_result` cover that. The runner cannot attach the field from gold.

The suite compiler sets the opt-in only when the question kind is `not_found`. `template_empty_result_is_schema` refuses an operator opt-in on the template. `admitted_null_cell_is_copied_and_top_level_null_is_schema` records the a267 contract correction: `oo_core::parse_json_value` accepts JSON null, a checked-object null is schema, and an admitted null cell is copied.

## Live TLS

`linux/live/out/oracle.txt` records the real `query-pg` binary, the post adapter, and the unchanged comparator. Zero-row `SELECT n FROM probe.slot WHERE n = 2` is query exit 0, adapter exit 0, comparator exit 0, verdict `match`, `returned_rows` 0. The contrary select `WHERE n = 1` is query exit 0, adapter exit 0, comparator exit 1, verdict `mismatch`, kind `rows`, `returned_rows` 1. Both query summaries are `succeeded`, `truncated` false, `rollback_confirmed` true, `socket_dropped` false, `end_audit_ok` true, `stop` null, and `sqlstate` null. The fixture is `probe.slot`. The requests do not name `dispatch.ticket`.

## Failed live builds

`linux/attempts/001/build-query-pg.stderr` ends in bindgen: `Unable to find libclang`. Exit 101. OOMKilled false. The retry extracts Debian `libclang-19-dev` onto the private cargo volume with the a222 `select-debs.rs` and `sha256.rs` tools. `linux/attempts/002/` then compiled `query-pg` and failed `cargo rustc --example a276_empty_result -- -D warnings` because `Slot.column` was never read. That field was removed. The successful container is recorded in `REPORT.md`.

## Unchanged boundaries

The comparator source was not edited. Its live and red mismatch reports use the existing `kind` difference. `oo-gateway` was compiled from the fixed archive and was not edited. No registry crate was added. The three bench `Cargo.toml` files and their locks were not edited. Help text and protocol ids were not edited.
