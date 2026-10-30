# Native library operator-reviewed PII reads

This is a read-scope grant, not PG SUPERUSER, a write grant, business-semantic
approval, or permission to bypass TLS, authentication, audit, or publication gates.

## Existing artifact contract

The native `oo.builder.effective-oos/v1` policy field contains only `id` and
`version`. The config schema's `Role.pii_access` is not a field of this document.
Do not insert it into an effective OOS or introduce a runtime bypass flag.

Use the existing `oo.builder.role-visibility/v1` per-role dataset/column allowlist.
`library_control.reviewed_documents` emits a PII column there only after its
explicit operator decision. The producer binds the review, effective OOS and
visibility hashes and the compiled Pack. Gateway rechecks those document hashes
and recompiles the Pack before mapping a listed declared PII column to
`PiiClass::Declared { explicit_allow: true }`. Classification alone never adds a
column to visibility.

## Operator input

The existing `library_build.py` config accepts:

```json
{
  "column_review_input": {
    "path": "/private/operator-review.json",
    "sha256": "<SHA-256 of the exact review file bytes>"
  }
}
```

The pinned file uses the current facts digest and actual project/role/principal:

```json
{
  "protocol": "oo.library.column-review/v1",
  "project": "library",
  "facts_sha256": "<current facts_sha256>",
  "review": {
    "project": "library",
    "role": "oo_r_library",
    "principal": "library_reader",
    "hidden_columns": [],
    "columns": {
      "app.items.id": {
        "pii": "none",
        "semantic_type": "identifier",
        "reviewer": "operator_full_read_scope",
        "label_approved": false
      },
      "app.items.phone": {
        "pii": "personal",
        "semantic_type": "phone",
        "reviewer": "operator_full_read_scope",
        "explicit_allow": true,
        "label_approved": false
      },
      "app.items.note": {
        "pii": "sensitive",
        "semantic_type": "free_text",
        "reviewer": "operator_full_read_scope",
        "explicit_allow": true,
        "reason": "User authorized read-only all business data; original candidate unknown, conservatively sensitive. This is not business-semantic review.",
        "label_approved": false
      }
    }
  }
}
```

These are synthetic examples, not actual customer field decisions. Generate one
decision per intended field from the actual facts and the operator's read-scope
authorization; there is no wildcard or inferred grant.

- Known `personal`, `sensitive`, and `restricted` retain their exact level.
- `explicit_allow` must be the JSON boolean `true` for declared PII. Missing,
  false, string, numeric and other values fail closed. For `none`, omit it or
  use `false`, matching the existing reviewed-policy adapter.
- An original `unknown` can become only `sensitive`, with an explicit per-field
  decision, nonempty `reviewer`, nonempty `reason`, and `explicit_allow: true`.
  Without that decision it stays pending and invisible.
- Preserve the candidate semantic type (including the existing `money` to
  `quantity` projection). Do not invent semantic labels to obtain read access.
- `reviewer` becomes `classification.by`; `confirmed` records this technical
  classification decision, not user review of business meanings. Leave
  `label_approved: false` unless there is separate descriptive approval.
- Hidden columns override decisions. Unreviewed columns are not added.

The original facts are unchanged. `effective-review-input.json` retains the
decision and reason; `review.json` records `pii_read_grants` with `source_pii`,
the reviewed level, reviewer and (for unknown) reason. The review digest binds
the input, and `business_owner_signed` remains false.

## Publication boundary

Release G13 accepts the candidate's sealed native role/column visibility for
reviewed declared PII. It still rejects unknown/missing PII and missing roles.
If a legacy `allowed_roles` restriction is present, it must match the current
role even for `pii: none`; native visibility cannot override it. G13 is not an
authentication decision or a substitute for the candidate/Pack seal.

`target.rs` already uses explicit logical-to-physical column bindings and
read-only reader checks; it has no PII classifier to relax.

The control API's ledger-to-review projection in
`tools/selfbuild-control-api/src/library.rs` still filters non-`none` decisions.
Use the existing pinned operator-review build path for this capability; the API
ledger path needs a separately reviewed adapter before it can produce the same
grant. Legacy single-table preview loading is also unchanged. Candidate-only
metrics (`certified: false`) leave G5/G11 `not_applicable`, not `pass`;
missing/malformed certification and certified metrics keep the strict gates.
Actual candidate staging/publication, database privileges,
tokens and running services are outside this patch. Tests use synthetic facts
and do not prove a live all-data release.

The pinned local build can prepare its Gateway input with
`tools/pg-builder/scripts/library_runtime.py`. Its envelope uses
`oo.operator.library-pack/v1`, `prepareLocalBuildPack` and
`management_identity: operator_local`; it never claims the control API authenticated it.
Gateway validates the included review digest and exact project/role/principal,
column PII/semantics/reviewer, explicit PII grant and hidden-column exclusions.
This is an operator-owned filesystem input, not an untrusted HTTP upload or a
token privilege claim. The usual Pack/native-binding checks still apply.
