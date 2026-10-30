# SB-01 / F19: binding validation and the missing trust adapter

Historical fallback contract: the runnable, authenticated test-scope continuation
is now documented in `BUSINESS-POSITIVE.md`. This standalone checker still does
not authenticate arbitrary JSON; the new path resolves the separate immutable
TLS-authenticated database ledger. Prior fallback evidence is not relabeled.

This delivery is the safe fallback allowed by Spec 9. It does **not** implement
positive business admission, complete F19, or issue a test/production certificate.
The current issuer infrastructure cannot safely establish the requested positive
path. Do not interpret a successful binding check as proof of execution/review.

## Confirmed trust boundary

`store::gate_body` authenticates the control DB administrator over TLS, but accepts
the generic `test_issuer` class and `pass` status from its caller. `gate_report`
stores a content certificate hash, class/status and detail, not a separately
authenticated execution/review issuer, observed result, snapshot, expiry, or
revocable issuer-policy version. `seal::authority_statement` covers a bounded
local fixture; it is not an authenticated business-owner statement.

Consequently, a reference to an old generic gate row, an identical JSON SHA, or
an `accepted/authenticated=true` field cannot establish business evidence.
No new approval switch, production issuer, signature scheme, or issuer registry
has been invented here.

## Executable changes

- `business_evidence::check_business_evidence` validates exact receipt coverage
  for G5a, G5b, G6, G10 and G11, against a supplied **independent** context.
  Successful validation still returns `outcome=refused`,
  `reason=business_issuer_unavailable`, `bindings_validated=true`,
  `authenticated_receipt_reference_resolved=false`, and both admissibility flags
  false. The returned JSON is not an intake accepted by any gate/store operation.
- `gate_record` refuses nonblocking generic `pass` claims when the stored
  candidate requires business evidence (or lifecycle proof for G12), before
  storing/replaying a gate certificate. Fail/not-run reports remain recordable.
- `store::assess` reapplies the existing `apply::preconditions` after maintenance
  overlays, for status, plan, enter-applying and commit rechecks. Historical
  generic test passes or stale `not_applicable` overlays cannot bypass current
  business requirements by calling plan/activate directly.
- `maintain` keeps the same fail-closed preconditions. Production admissibility,
  owner authentication and business signing remain false. Catalog-only candidates
  are not newly treated as certified business assets.

## Interface for the candidate execution owner (Dirac)

These are contracts, not edits to Gateway/SDK/CLI. No assumption is made that
their current implementation already exposes them.

| Field | Required independent origin |
| --- | --- |
| `candidate_id`, `candidate_digest` | Stored coordinator seal; digest is the existing `source_hash`, not merely Pack digest |
| Pack identity | Existing sealed `pack_digest`/`manifest_sha256`; already included in `source_hash` |
| `semantic_definition_digest` | Canonical entire metric/example definition, or pinned eval plan; includes SQL, unit, grain, scope, tests, review metadata |
| `dependency_digest` | Canonical entire effective OOS in this conservative slice; not a caller-supplied arbitrary dependency hash |
| `role`, `policy_digest` | Role from stored visibility; canonical SHA256 of the entire visibility including membership, policy version and limits |
| `data_vector_digest` | Actual candidate-execution snapshot vector supplied by the executor; no catalog/version alias, no fabricated current vector |
| `source_coverage` | Actual independently measured coverage/scope, compared exactly; not inferred from a query's successful exit |
| `sql_or_plan_digest` | Metric SQL/tests, SQL plus independent control-total plan, example SQL/uses, pinned eval plan, or bound semantic-diff review plan |
| `execution_capability_version` | Pinned candidate credential/executor protocol version used for the execution |
| expected/observed digests | Independent oracle registry versus actual executor result; neither side may be generated from the other's claim |
| receipt reference | Immutable authenticated issuer record resolving the complete body; not a bare self-hash or caller JSON file |

Gateway must bind its execution/query receipt to the same candidate, target role,
policy and **actual** data vector throughout all queries. Any drift must fail or
start fresh execution; it must not rewrite the result header to the new vector.
The coordinator must re-resolve current issuer policy/revocation and snapshot
validity at plan/CAS, not only at receipt import.

Canonical digests use the existing coordinator `canon::canonical_sha`, UTF-8
JSON, sorted object keys, no whitespace. Arrays retain order. Share canonical
bytes or cross-language golden vectors before integration; do not assume another
JSON serializer's numeric representation is identical.

## Validation-only receipt contract

Schema: `oo.release-coordinator/business-evidence/v1`.
`BusinessEvidenceContext` is the Rust interface; see `src/business_evidence.rs`.
Unknown fields, duplicate required assets/references, incomplete receipt sets,
and mismatched expected-result registry keys are refused.

Every receipt includes:

```text
evidence_id, evidence_kind, evidence_schema
issuer, issuer_scope, issuer_policy_digest
authenticated_receipt_reference: {issuer, receipt_id, body_sha256}
candidate_id, candidate_digest
semantic_definition_digest, dependency_digest
data_vector_digest, source_coverage, role, policy_digest
sql_or_plan_digest, execution_capability_version
expected_result_digest, observed_result_digest
started_at, completed_at, expires_at, outcome, unresolved_count
gate_id, asset_id
```

Times are integer Unix seconds, supplied from an independent verifier clock.
Require `started <= completed <= now < expires`, an active issuer policy covering
the execution interval, and at most one hour from start to evidence expiry.
Only `outcome=pass`, zero unresolved items and exact independent expected/observed
digest equality pass the binding checks. Tolerances must be part of the pinned
independent comparison plan/result, not a caller-provided waiver.

The full current issuer-policy claim is also bound, including class, issuer,
project, candidate, role, gate coverage, source coverage, policy version,
validity and revocation. Only test scope is currently modeled. This claim is
**not authenticated by the validator**. Body SHA excludes the reference itself
and protects integrity only; reference resolution/authentication is still absent.

Receipt requirements are G5a/G5b/G11 per certified metric, G6 per example, G10
per required eval. Uncertified metrics are not silently counted as certified.
Removing assets cannot satisfy the original independent expected-result registry.
No signatures or business approvals are synthesized from `owner/verified_by`.

## Exact external dependencies before a positive path is safe

1. A separately authenticated execution/review issuer identity and installation
   trust policy, scoped to project/candidate/role/gate/source coverage, with live
   revocation and expiry. Control-admin login alone is not this identity.
2. An immutable receipt resolver (verified signature or authenticated ledger/API)
   returning the entire body and authenticated issuer, with no authority supplied
   by the same request being admitted. Current `gate_report` is insufficient.
3. Candidate executor receipts from Dirac binding candidate/Pack, role/policy,
   actual snapshot vector, exact SQL/plan, execution capability and result/query
   identity. A caller-supplied vector or execution claim is not a substitute.
4. Independent oracle/eval-plan registration and an authenticated semantic-review
   principal. Model-authored expected results and self-declared owner names do
   not satisfy this dependency.
5. Durable storage and plan/CAS revalidation of these bindings and current trust
   state, including re-execution on changed data vectors. No production issuer
   is provided by the existing fixture authority.

These require coordination with the relevant trust/execution owners. This change
does not write their domains or weaken the current gates to avoid coordination.

## Local tests

`tests/observations/business_evidence.rs` tests all five well-bound claim kinds
but explicitly expects **no admission**, plus definition/SQL/unit/scope,
data-vector, role/policy, expected/observed, issuer policy/scope/revocation,
expiry, reference integrity, missing/duplicate evidence and approval-field
injection refusals.

`tests/run_business_guard_fixture.py --evidence NEW_DIRECTORY` creates only a new
private loopback PostgreSQL cluster and CA/leaf TLS chain, installs the real
coordinator under `oo_release_admin`, stages a real compiler-produced metric/
example/eval candidate, and tests authenticated generic pass refusal. It then
inserts explicitly labeled historical generic attack fixtures and stale
maintenance overlays, proving plan/activate remain blocked and the pointer is
unchanged. It does not execute business SQL or authenticate a business issuer.
Its raw command exits and receipts are retained; its own DB is stopped/removed.

No customer database, public endpoint, cloud, model, existing container, or shared
installation is used. This delivery does not edit the previous 11 semantic files
or their historical receipts. F20's newly assigned API/control-script changes
are independently owned and are not part of this release-coordinator delta.
