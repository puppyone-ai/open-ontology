# pg-onboarding contract

This file states the current behavior of the standalone `tools/pg-onboarding` tool. It corrects two false statements in the historical a261 contract: a failed outcome no longer claims `unchanged_by_this_tool` after a child process has started, and duplicate JSON keys are refused. Inspect and apply field schemas are unchanged. No field was added to a successful outcome. This file does not accept a parent unit, semantic publication, authentication, natural language, or a release.

The tool remains a standalone workspace. It has no path dependency on a product crate. Pins stay `serde =1.0.229` with `derive` and `serde_json =1.0.151`. There is no `cargo update`.

## Failure state

`database_rollback` is false. `database_release` is false. The tool does not drop roles, drop schemas, roll back, or publish.

A deadline outside 1..=180000 is rejected in config validation, before the run directory is created. No outcome file is written. A duplicate manifest is refused before apply starts, so no apply outcome is written either.

A failure outcome uses `database_state=unchanged_by_this_tool` only when that outcome is written and no child process has been spawned in that phase. Decision and review refusal before the first `Command::spawn` are in this set.

After `Command::spawn` has returned for any child in the phase, a failure outcome uses `database_state=changes_may_have_been_applied`. The tool does not prove that the child left the database untouched. A later failing child does not erase earlier step files or an external side effect.

Files already written stay in place. The tool's stderr line remains `pg-onboarding: <step> <static detail>`. The line does not include child stdout, passwords, or marker text.

Success outcomes are unchanged: inspect `awaiting_review`, apply `ready_for_trusted_query`, with `database_state=child_compilers_applied_no_release` on those success objects. `database_rollback` and `database_release` stay false there too.

## JSON documents

Operator config, the inspect manifest, the inspect outcome, reviews, decisions, and visibility files are parsed by the tool's duplicate-rejecting visitor before `serde_json` builds the value. The same visitor parses child JSON, with the child byte cap below.

A repeated key is refused at the root and inside every nested object. That includes role objects, generation fields such as `source_generation` and `source_cut`, manifest objects, and decision objects. Escapes are decoded before the key compare, so `"a"` and `"\u0061"` are the same key.

Refusal happens before the subprocess that would consume the document:

- config, manifest, and outcome-file errors return before `apply_in` spawns
- decisions are refused before `review-pg-draft`
- reviews are refused before `review-pg-draft`
- a visibility file is refused before that role's `role-pack`

Limits for those operator documents:

- 1048576 bytes
- depth 32, counting the root value as depth 1
- 50000 JSON values
- a non-whitespace byte after the value is refused
- whitespace after the value is accepted

Over the byte, depth, or node cap, the detail is `budget`. Duplicate keys, trailing bytes, and other syntax failures use `shape`. A document inside the cap that is not the config schema still uses `config`.

Child stdout and other captured compiler JSON use the same duplicate, depth, node, and trailing-byte rules. Their byte cap remains the existing capture cap, 8388608. This tool does not lower that capture cap.

`serde_json` is not the duplicate-key authority for these documents. It only materializes a document the visitor has already accepted.
