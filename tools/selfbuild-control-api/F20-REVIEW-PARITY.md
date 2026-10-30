# F20 reviewed Library projection

## Fixed authority contract

No new management capability, HTTP request field, IAM, token or publication
permission is introduced. Classification remains a ReviewLedger decision:
`pii`, `semantic_type`, `state=confirmed`. Its reviewer comes from the existing
verified identity -> configured principal/object scope -> real ledger path.
`business.record` / `classification.confirm` do not grant sensitive reads.
Caller `explicit_allow`, reason, role names and `visible_columns` do not authorize
PII. `visible_columns` remains a membership ceiling, not an authorization source.

The existing operator-owned Library config's `column_review_input` supplies the
separate read authorization for both local and HTTP compilation. The adapter
reuses `library_build.pinned_review`: regular non-symlink file, exact byte SHA256,
`oo.library.column-review/v1`, exact project and current facts digest.
The HTTP Library config is itself pinned by `LibraryCfg.config_sha256`; no
request can choose the file. Local config/files are an operator filesystem trust
boundary, not authenticated HTTP uploads.

The existing document's `review.project`, `role`, `principal`, `hidden_columns`
and exact per-column `columns` entries bind the scope. There are no wildcards.
The HTTP review target must equal the pinned policy target. Every selected
column must be in that policy when it is installed. Hidden/granted conflicts,
unknown column identities and mismatching classifications fail closed.

For each non-`none` column the operator entry must contain:

```json
{
  "pii": "personal",
  "semantic_type": "phone",
  "reviewer": "read-scope-operator",
  "explicit_allow": true,
  "read_grant_expires_at": 1798761600,
  "label_approved": false
}
```

`read_grant_expires_at` is an integer Unix timestamp in seconds, checked against
the executing host's clock on every projection. Missing, noninteger, boolean,
or elapsed values are refused. Existing PII policy files without this new field
must be explicitly renewed and re-pinned by their operator, not auto-upgraded.
For `none`, omit `explicit_allow` or use false. Existing no-PII ledger-only
builds retain their prior behavior; installing a column policy narrows them too.

Known personal/sensitive/restricted classifications cannot change level.
An original unknown may become sensitive only with the pinned operator's
nonempty per-column reason and explicit grant, plus matching ledger
classification for HTTP. Reasons in HTTP classification payloads are not used.
Original facts remain unchanged. The existing money -> quantity projection
and descriptive/semantic approval checks remain intact.

Read authorization does not replace classification reviewer or approve labels,
grain, metrics, examples, dictionary mappings or business semantics.
`read_authorization` records the policy byte digest, facts digest, exact scope,
grant author and expiration in `effective-review-input.json` (bound by the
producer review digest) and `review.json`. The existing per-column explicit
grant remains available to native consumers; PII never becomes `none`.

## Bindings and replay

Library project input hashes now include role-policy bytes, visible/hidden
membership and automatic-review policy, in addition to the existing pinned
Library config/script/manifest. Old builds must be recreated/reviewed; do not
rewrite historical inputs to make the new hash match.
The frozen semantic owner's S5 scope logic remains in place, with the operator
column-review pin appended to its permission hash so grant changes invalidate
that authorization context too.

Compilation verifies actual merged facts against every stored member's complete
candidate/source/schema/generation binding, then verifies current ledger heads
and this build/object's decision keys as before. Python independently verifies
facts/drafts and requires the matching S6 merged document for ledger reviews;
direct offline projection can derive it from checked facts/drafts. Mutable policy files,
role scope and facts cannot drift between review and compilation.

Cached HTTP GET and compile replay recheck current ledger, pins and grant expiry
and immutable projection outputs. An existing Pack record is not authorization
to bypass these checks. Native OOS-tree equality, complete required assets,
required metrics and the native role Pack compiler remain mandatory.

This is compile/retrieval-time authorization. It does not issue Gateway tokens,
change PostgreSQL grants, publish, or revoke an already published runtime policy.

## Evidence

`tests/run_f20_owned.py` starts only a new named native PG cluster, copies and
hash-pins the chosen ReviewLedger test executable, uses formal Axum routes with
the production identity adapter against a pinned TLS identity-contract fixture,
and invokes the actual S6-S10 review/Pack path. It compares complete effective
OOS, visibility, native artifact and Pack digest with local compilation.

It is NOT a real IdP/customer HTTP acceptance test. S0-S5 are retained synthetic
fixtures; there are no model/source calls. The runner stops its own PG/server.
Installation ceilings are explicit and match the Library visibility limits;
the compiler's budget and safety checks are not removed.

See `F20-ROOT-INTEGRATION.md` for out-of-domain dependencies. Do not mark the
whole F20 or source-onboarding convergence checklist complete.
