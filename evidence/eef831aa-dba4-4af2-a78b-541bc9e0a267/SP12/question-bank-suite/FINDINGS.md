# Findings

Reviewer: the same session that wrote the crate. Scope: the frozen compiler behavior and the boundary tests impl-4 already ran. The review reads `benchmarks/question-bank-suite` and the preserved logs. It did not launch another test container.

## Admission and selection

`compile_document` serializes the embedded bank object and calls `oo_question_bank::evaluate`. A bank `Err` becomes token `bank` at `/bank` plus the bank pointer. Decimal failures stay on that path. There is no separate decimal token. After evaluate, the compiler reparses the bank report and checks the manifest, agent-view, and scorer-view hashes plus bank identity.

`select_group` keeps every question whose asker role equals the selected role, in bank order. A different `as_of` inside that role is token `group` at `/selection/as_of` and writes nothing. An empty role is token `group` at `/selection/role`. More than 32 selected questions is token `budget` at `/cases`, before `plan_question`. Other roles are listed in `excluded_question_ids` and are absent from the suite. `fully_approved` maps to annotation `score`. `needs_reannotation` stays `needs_reannotation`. k is parsed as 1 through 8.

`clarify` and `refuse_permission` are token `unsupported` at `/bank/questions/{index}/expected/kind`. The whole compile stops. impl-4 `unsupported_kind_refuses_the_sibling_too`, `missing_gold_refuses_the_whole_group`, `relative_tolerance_is_not_exact_gold`, `more_than_32_cases_refuses_without_truncation`, `mixed_as_of_refuses_the_whole_role`, `bad_manifest_refuses_before_a_directory`, and `bad_decimal_stays_a_bank_refusal` passed.

## Gold and the runner

Supported `table`, `scalar`, and `top_k` copy declared `gold_sql` into the trusted request and copy `typed_gold` into `gold.json`. `ordered` true becomes row comparator `ordered`. `ordered` false becomes `unordered`. A relative tolerance is token `gold`. Column types are `bool`, `int64`, `decimal`, and `text`. Binding columns are derived with ordinals. The template header carries budgets and no columns.

`not_found` compiles with row comparator `ordered` and an empty column list. The fixed query-result adapter emits `kind` `rows` with an empty row list for that gateway result. sql-runner then records mismatch. The proof keeps both trials in the denominator and expects exit 1. The gold file stays `kind` `not_found`. That limit is stated in `CONTRACT.md`. The compiler does not drop the id and does not rewrite the gold into an empty row set.

`reject_secret_and_gold` scans the password bytes before `create_dir`. Gold SQL lives in `request.json`. Typed gold lives in `gold.json`. The subset agent view is built from the bank agent view of the planned ids. The stdout report is the planned manifest without the `files` object, plus `manifest_sha256`.

Executables are hashed from `symlink_metadata` and a file read. The compiler does not spawn them. A symlink executable is token `path`. impl-4 `executable_bytes_are_hashed_and_not_spawned` and `symlink_executable_is_refused` passed.

The suite directory is `create_dir` of one missing final component, mode `0700` on Unix, files `create_new` mode `0600`. An existing path is token `out_dir` and is left in place. A later write failure removes the new directory only when the path has at least four components.

## Preserved failures

`linux/impl/test.stderr` is `error[E0382]: borrow of moved value: root` in `tests/support/mod.rs` at the `World::new` and `with_executables` literals. `linux/impl/clippy.stderr` is `too_many_arguments` on `report_value` (8/7) and `needless_return` in `src/lib.rs`. The help arm is now the match result. `report_value` takes the built manifest value and the manifest hash.

`linux/impl-2` tests passed, including the proof in 25.70s. Clippy then failed on `document_with_hash` (8/7) and `shell` (11/7) in `tests/support/mod.rs`. The current helpers fold the dataset and suite into one pair and pass a `Shell` struct. No `allow` attribute was added.

`linux/impl-3` tests passed, including the proof in 25.67s. Clippy then failed on `manual_strip` in `tests/compile.rs` for the bad-manifest nibble flip. The current test uses `strip_prefix`. That test passed again on impl-4.

impl-4 `linux/impl-4/launch.sh` still prints `refusing to overwrite impl-2 console.log` while `EVIDENCE` and `NAME` are impl-4. The guard checks the impl-4 console path. The script is the one that ran. It was left as the raw launcher.

## Contract wording

`CONTRACT.md` says `oo_core::parse_json_value` rejects JSON null. `crates/oo-core/src/json_scan.rs` accepts the literal `null`. The compiler rejects null through `object`, `req_str`, and `exact_keys` with token `schema`. A top-level null fails at `/`. Typed gold is copied as the bank emitted it, so a null cell inside admitted gold is not rewritten. The contract sentence is broader than `parse_json_value`. The impl-4 tree was left unchanged after the green run, so this wording stays in the tested contract. Behavior under test is the schema rejection on checked objects.

`select_group` uses pointer `/templates` when a template role is absent from the bank. Field checks inside one template use `/templates/{role}`. `CONTRACT.md` requires token `template` for a missing or extra role and does not require the role key in that pointer.

## What this review leaves open

The compiler does not open a database connection. The proof's query-pg program is a controlled test double. The live adapter mismatch on `not_found` remains part of the trial count. This review does not mark B-D07 accepted, does not treat the hand bank as a 300-question bank, and does not claim a human annotator or a customer holdout.
