# Findings

Session `eef831aa-dba4-4af2-a78b-541bc9e0a286`. Read with `REPORT.md`. The product tree was left as it stood at `c551afea55d5a9164760878a5d6a76835b869875`. The failures below are in the review oracle and in one image component. They are preserved under `linux/smoke/attempt-001` through `attempt-005`.

## Fresh-bank oracle

`attempt-001` stopped in `tools/lock_subset.rs` with rustc E0119. `Package` had both `#[derive(Default)]` and a manual `Default`. The derive was removed. The same run then failed `oracle/fresh_bank.rs` with E0308: a version check passed `&str` where `eq` takes `&Value`. The later lock subset is 575 registry triples, 0 missing, 0 mismatch.

`attempt-002` used `as_of` `2026-10-06T00:00:00Z`. `attempt-003` used `+00:00`. `oo_core` normalizes a zero offset to `Z`, and question-bank `canonical_as_of` requires the original text to round-trip at 25 characters with a nonzero offset. The bank token is `as_of` at `/questions/0/as_of`. The green bank uses `2026-10-06T00:00:00+08:00`. The other role uses `2026-10-05T00:00:00+08:00`. Selection `as_of` is that same string. Tuple `as_of` stays the date `2026-10-06`.

`attempt-004` built gold text cells as gateway `{"kind","value"}` objects. Question-bank cells are presence objects. The token was `schema` at `/questions/0/expected/typed_gold/rows/0/0/presence`. The green bank uses `{"presence":"value","value":...}` and a decimal cell of coefficient `450`, scale 2. The query double still sends gateway `{"kind":"text","value":"4.50"}`. The adapter stdout then carries coefficient `450`.

`attempt-005` compared `planned[].as_of` with the timestamp. The compiler's planned rows are `annotation`, `id`, and `index`. `selection.as_of` already matched. The green oracle checks `index` and `annotation`. `rv-note` remains `needs_reannotation`.

## What the extension does on this archive

`linux/smoke/seam-marker.txt` is `fixed=present parent=absent`. `oracle/boundary.rs` compared the two built adapters. Omitted and explicit `rows` match the parent bytes. The parent refuses a header that contains `empty_result`. A clean opt-in zero-row result is the `not_found` object. Truncated, failed, and `max_rows` 0 zero-row results match the parent empty table. A contrary row stays rows.

The fresh absent gold file stays `kind` `not_found` with no row list. Its binding carries `not_found_if_complete_empty`. The table binding omits the field. The runner's first adapter stdin line parses equal to that binding. The header line does not contain `expected` or the SQL comment.

The runner report for the fresh bank is 5 questions, 10 trials, 8 passes, 0 failures, 2 reannotations, `passed` false. The two `rv-note` trials stay `needs_reannotation` with comparator verdict `match`. The oracle required process exit 1 from sql-runner. `fresh-bank.exit` is 0.

`template_empty_result` on the operator template refuses as `schema` at `/templates/review_north/binding_header/empty_result`. The compiler does not infer the opt-in from an empty column list. A coefficient `0450` refuses as `bank` while the manifest hash is still a dummy. A top-level null is `schema`. An admitted null cell is copied. `refuse_permission` refuses the whole compile. A changed manifest hash refuses before the output directory exists.

The written agent view has five questions and does not contain `gold_sql`, `typed_gold`, `rv-south`, the control canary, or the annotation canary. The scorer side of admission still carries gold and the control total. The compiler report does not contain the SQL comments, the cell canaries, or the fixture password constant.

## Live query-pg

`linux/live/out/zero.query.stdout` is one summary line from the real binary: `succeeded`, `truncated` false, `returned_rows` 0, `rollback_confirmed` true, `socket_dropped` false, `end_audit_ok` true, `stop` null, `sqlstate` null. The statement is `SELECT v FROM review.cell WHERE v = 9`. Adapter stdout is the `not_found` object. Comparator verdict is `match`, exit 0.

`WHERE v = 4` returns one int64 cell `4`, adapter kind `rows`, comparator verdict `mismatch` on path `kind`, exit 1. `probe.exit` is 0. `secret-scan.exit` is 0. No PEM private key is in the evidence tree. The live password files stayed under `/tmp` and were not copied here.

`live_bytes` prepended omitted, `rows`, and opt-in headers to that captured zero-row stdout. Omitted and `rows` match the parent adapter. The opt-in matches the `not_found` object. The contrary query stdout was left out of that replay because a nonempty result must stay rows.

The first `query-pg` offline fetch exited 101. The online fetch exited 0 and the following offline build exited 0. Libclang was extracted from Debian trixie arm64 packages into the private cargo volume. `libclang.exit` is 0. `LIBCLANG_PATH` was unset before the probe.

## Locks and clippy

Every registry triple in the five bench locks is an equal root-lock triple. The three seam locks are byte-identical to the parent archive. The seam commit name diff for `Cargo.toml` and `Cargo.lock` is empty. a207 remains the only writer of the connector manifest and the root lock. This review wrote none.

`cargo clippy --version` exits 1 on the pinned image until `rustup component add clippy --toolchain 1.94.0`. After that add, clippy `-D warnings` on the three crates exited 0. The component add is an image gap. The crate sources compiled clean.

## Unchanged boundaries

The result comparator source was not edited. The live mismatch is the existing `kind` difference. `oo-gateway` was compiled from the fixed archive. The probe example was added only in the build copy of that archive. `lock-before` and `lock-after` match. Help text and protocol ids were left as archived.
