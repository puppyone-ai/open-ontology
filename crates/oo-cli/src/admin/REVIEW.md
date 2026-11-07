# Owner self-review

`oo review --input input.json --out reviewed.json` opens the existing ratatui
terminal screen. Install `selfbuild-budget` beside `oo`. Inputs, decisions,
checkpoints and outputs must be regular private (0600) files. No source
connection, model call, credential lookup, authentication or publication occurs.

The input is G's `oo.builder.reviewed-community/v1` command **without** `review`:
`draft`, `effective`, `visibility`, `orphan_threshold`, and `ceilings`.
The draft must have no unresolved items. The effective OOS and role visibility
are operator-pinned inputs, not inferred grants.

Each entity (including its key/display selection), table/column description,
relationship, metric and column classification requires an explicit decision.
An empty answer and `--yes` do not accept anything. Edit accepts a JSON object
containing only the fields displayed in the prompt; physical identities and
visibility grants cannot be edited here. Columns need an explicit PII level
and semantic type, with no automatic `none` fallback.

Reject records a reason and blocks the **whole candidate**. It does not silently
delete a table, omit a classification, or publish a partial registry. Correct
the input and start another review to reconsider a rejected candidate.

After edits, the real Rust Builder recomputes connectivity. The owner separately
confirms the G14 summary and supplies a reason. Missing FK mappings, stale summary
hashes and invalid registry/Pack contents fail compilation even if approved.
Model-supplied certification never substitutes for this decision.

An interrupted TTY review retains `<out-stem>.pending.json`. Rerunning with the
same input/output resumes saved item decisions; the input digest must match.
The G14 summary is always recomputed and reconfirmed. A file lock serializes
reviews of the same output. Existing output files are never overwritten.

## Non-TTY decisions

Use `oo review --input input.json --review-file decisions.json --out reviewed.json`.
The same validation and compiler run in both modes. The decision file is an
operator assertion, not proof of a separately authenticated reviewer:

```json
{
  "protocol": "oo.owner-review/v1",
  "input_sha256": "<SHA-256 of compact, sorted-key input JSON>",
  "reviewed_at": "2026-11-04",
  "items": {
    "entity/<stable-id>": {"action": "accept"},
    "description/<stable-id>": {"action": "accept"},
    "relationship/<stable-id>": {"action": "accept"},
    "metric/<metric-name>": {"action": "accept"},
    "classification/<dataset>/<column>": {
      "action": "edit",
      "replacement": {"pii": "personal", "semantic_type": "person_name"},
      "reason": "Owner checked the column definition"
    }
  },
  "g14": {
    "summary_sha256": "<hash of recomputed connectivity summary after edits>",
    "action": "accept",
    "reason": "Owner checked FK mapping, conflicts and isolated entities"
  }
}
```

Every actual item must occur exactly once; the example is not a review template
that auto-accepts new items. The persisted TTY decision document can be replayed
as the file input. TTY and file modes produce identical reviewed output.
Use the Rust bridge's `review` operation to compute the final summary hash;
do not hash Python floating-point JSON representations without checking parity.

## Publisher handoff and limits

`oo.owner-reviewed-registry/v1` contains `review_decision.reviewer=owner`,
`review_decision.mode=self`, the complete decisions, the reviewed compiler
`command`, and its `compiled` result (effective OOS and role Pack producer).
The publisher consumes `compiled` through its authenticated publication path;
this envelope does **not** grant publication authority or satisfy other gates.
`published=false` and `authentication=not_claimed` are intentional.

The existing bounded `oo build` description-generation artifact is not this
community input. This command alone does not convert that artifact, deploy a
Gateway, issue a token, or complete S4-A-02.

Validation: `cargo test -p oo-cli --locked` and
`python3 crates/oo-cli/src/admin/test_review.py --run-id <new-id>`.
The latter uses the real compiler and PTY with a deterministic model replay;
it is not a live-model or database acceptance test.
