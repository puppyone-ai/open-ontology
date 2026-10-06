# Review: outcome honesty

Scope: `tools/pg-onboarding` failure outcomes in commit `50d4e705a2bb4ba014919ef1e071da065cf8e9b7`, checked against `linux/red-attempt-4` and `linux/green`.

The phase flag is a `Cell<bool>` passed into `run_child`. It becomes true only after `Command::spawn` returns `Ok`. A secret refusal returns before `spawn`. A deadline outside 1..=180000 fails inside `validate` before `create_dir`.

`linux/red-attempt-4` shows the unrepaired archive still writes `unchanged_by_this_tool` after an init-pg timeout, after a catalog failure that follows import, and after provision fails once install has written `side-effect-marker-ok`. `linux/green` writes `changes_may_have_been_applied` for those three, with `database_rollback` false and `database_release` false. The marker file remains. Process stderr is the static step line.

`preflight_deadline_refuses_before_run_dir` passes on both trees. The fresh directory is absent, so there is no outcome to misread.

Duplicate decisions write an apply outcome with `unchanged_by_this_tool` and do not create `review-pg-draft.stdin`. Duplicate manifest returns before `apply_in` and writes no apply outcome.

This review does not treat `ready_for_trusted_query` as publication, authentication, natural language, or a release. The tool does not roll back.
