# Controlled test-scope business admission

The native v2 extension is documented in `BUSINESS-NATIVE.md`. It uses actual
publication/current-generation authority and bounded aggregates rather than the
v1 ordinary-table data courier. The v1 scope and historical proof below remain.

This is the runnable continuation of the prior validation-only fallback.
The old fallback receipts remain historical evidence; arbitrary JSON still
cannot pass `check_business_evidence`. The positive path resolves newly installed,
authenticated, append-only database records, not the caller's `test_issuer=pass`.

## Reused product mechanisms

- Coordinator TLS connection/CA verification, stored candidate seal and G8
  compiler recheck, gate summaries, deployment flags, install advisory lock,
  plan hash, active-pointer CAS and audit events.
- PostgreSQL login authentication: `session_user` and `current_user` must match
  the registered issuer; no new tokens, signature scheme or general IAM.
- Installation-side private-file SHA pin, as used by the review-ledger pattern.
  The existing review ledger's `authority=not_verified`/`authenticated=false`
  records and a self-declared component authority are not relabeled as trusted
  business execution. OIDC identities are not reimplemented.

`oo_release_business_v1.sql` is an additive migration. It neither edits the
original control migration/hash nor changes production authority.

## Small, explicit supported scope

One candidate, one control/Agent DB, one target role, up to eight ordinary
non-RLS tables, 16 columns/table, 1,000 rows/table and 1 MiB per table observation.
Only builtin integer/numeric columns and generated `SELECT count(column) FROM
schema.table` or `SELECT sum(column) FROM schema.table` are supported.
The generated target SQL must exactly equal the stored metric/example SQL;
control-total SQL must exactly equal the separately registered oracle plan.
Eval suite IDs are the actual `require_eval` IDs and have pinned target/oracle
plans. Metrics require the supported one-result-row test with empty parameters.
No SQL supplied through a receipt is executed. Unsupported SQL, NULL aggregate
results, RLS, cross-database execution and oversized observations fail closed.

This deliberately bounded test adapter does not replace Gateway's SQL parser
or implement a second general query system. Existing nonbusiness gate checks
remain independent, and no required asset list is cleared.

## Installation and separate authorities

The operator supplies a mode-0600 regular policy file and a separate SHA pin in
the installed CLI config. `business-install` request JSON contains no file path
or replacement pin. The policy binds:

- project/candidate and the complete coordinator candidate seal digest;
- complete effective-OOS and visibility hashes, target role;
- four distinct preprovisioned DB logins: executor, oracle, evaluator, reviewer;
- exact metric/example/eval IDs, target and independent oracle relation/column/
  aggregate plans, selected relations/columns and authoritative generation table;
- fixed `test_scope`, policy revision, expiry, receipt TTL (1..3600 seconds), and
  `business_execution_capability()` (implementation-bound adapter version).

The installer does not create roles, grant business data access, or register
itself as a business owner. Actors cannot be superusers, bypass-RLS, replication,
create-role/database users, members of privileged roles or the control admin.
Actors must lack DML rights to the scoped data/oracle/generation tables.
The target role and actual table/column ACL, membership identities/revisions,
role OIDs/validity, schema and capture generation are part of the observed vector.

Expected trust deployment: the privileged installer/control credentials remain
private. The three execution credentials are held by the controlled executor;
reviewer credentials are held separately. A user possessing a registered issuer
credential is that test authority, not a production/customer business owner.
The schema owner/cluster administrator is the installation trust root, not an
adversary the append-only triggers pretend to contain.

## Actual execution and immutable evidence

The coordinator holds SHARE table locks on all scoped data/oracle tables and the
generation table while the three independent TLS sessions execute their pinned
plans. Execution sessions are read-only, use `pg_catalog` search path and bounded
statement timeouts. Target execution/evaluation assume only the sealed target
role; the independent oracle uses its own role and different relation.

Each actor appends its full observation with policy/run ID, candidate/definition/
policy/data-vector binding, SQL hashes, actual scalar results, start/completion/
expiry and capability version. A database trigger sets the real `session_user`,
OID and server record time, checks the exact actor-kind scope, policy expiry
and revocation. RLS limits policy/receipt visibility to registered actors.
UPDATE/DELETE/TRUNCATE are rejected; no overwrite of a failed or expired run.

`business-review` uses a fourth authenticated login and an explicit digest of
the exact execution/oracle/eval receipt bundle. Missing observations, mismatched
actual versus independent expected scalars, wrong bundle or wrong login cannot
be reviewed. The immutable review references the exact three observation hashes.
It says `business_owner_signed=false`.

On resolution, all five business gates receive per-asset evidence views:
candidate/definition/dependency/data-vector/role/policy, source coverage,
SQL or review plan, expected/observed result digests, timestamps, issuer scope,
capability and authenticated database receipt references. Raw caller claims do
not enter this path. Both observation and reference hashes are recomputed.

## Plan and CAS

Status, plan, enter-applying and commit resolve the current private policy,
revocation, issuer identity/scope, exact receipts, independent comparison and
review. They reobserve the role-projected data, schema and capture generation.
All data table locks last until the transaction's CAS/commit; content changed
and then restored still invalidates because the capture generation advances.
The plan hash contains all immutable evidence hashes and the policy pin.
Expiry is checked again in the pointer UPDATE statement, bounded also by role
credential validity. Public/generic gate failures and conflicts are not hidden.

`maintain` consumes the same resolver for its initial business preconditions
only when the target is the same database, logical/physical bindings are identical
and there is no native remapping. Durable preconditions are never
rewritten to `not_applicable` to obtain a pass. Final plan/CAS rechecks remain
mandatory. Cross-database maintenance is still refused for this adapter.

Policy revoke is append-only and serialized with plan/CAS by the existing
install lock. Installing a different policy while the old one is unrevoked is
ambiguous and refuses; revoke the old policy explicitly and produce fresh
execution/review evidence. No implicit carry-over from old runs.

Production admission and all customer/authenticated-owner external gates stay
false. Successful test-scope activation yields the existing
`test_scope_not_production` certificate class.

## Existing CLI entry points

All commands use `release-coordinator COMMAND --config PRIVATE_CONFIG`.
The standard connection config remains host/port/database/user/password_file/
ca_file. `business-install` additionally requires
`business_policy: {"path": "...", "sha256": "..."}` in that private config.
`business-execute` additionally requires `business_actors` with executor/oracle/
evaluator standard login configs for the identical database. The review command
uses a separate reviewer login config, not a control-admin config.

```json
{"protocol":"oo.release-coordinator/business-install/v1"}
{"protocol":"oo.release-coordinator/business-execute/v1","policy_sha256":"PIN","run_id":"run-1"}
{"protocol":"oo.release-coordinator/business-review/v1","policy_sha256":"PIN","run_id":"run-1","execution_bundle_sha256":"EXECUTION_BUNDLE"}
{"protocol":"oo.release-coordinator/business-revoke/v1","policy_sha256":"PIN"}
```

The Rust API exports the same four operations and capability identifier.
Review is an explicit action; no automatic customer business signature is made.

## Minimal interface request to the candidate/capture owner

The current adapter consumes an installation-pinned ordinary relation
`generation_relation` with `dataset text` and `generation bigint`.
Every included dataset must have exactly one positive generation. The trusted
capture writer must advance it atomically with DML (including truncate); no
execution/review actor may update it. The fixture supplies a dedicated capture
table plus transaction triggers, not a fake vector header.

Root/Dirac/Agent-DB owner must map the real authority to that contract (or agree
an equivalent locked API). Candidate digest remains coordinator `source_hash`;
role/policy digest remains complete visibility; Pack digest remains separate.
Cross-database/Gateway candidate execution needs a trusted snapshot/capability
adapter preserving the same immutable receipt and final-CAS semantics.
No Gateway/SDK/CLI/capture-domain files were edited in this delivery.

## Executable local proof

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tests/run_business_guard_fixture.py \
  --positive --evidence /private/new-release-positive-evidence
```

The runner creates and removes only a new loopback PostgreSQL cluster and local
CA/leaf TLS chain, builds the real CLI, uses four independently generated
credentials, runs actual data/oracle SQL and invokes actual CLI install/execute/
review processes. Evidence includes the full anonymous policy files, immutable
ledger rows, positive gate/plan/activation and negative recheck receipts.
Credentials/configs are temporary private files and are not output artifacts.

The proof covers independent oracle mismatch, wrong issuer/review bundle,
immutable rows, actor spoofing, data drift between plan and CAS, content restored
with advanced generation, issuer-scope changes, policy revocation and expiry
between plan and CAS, then a fresh successful test activation. The original
validation-only and generic-pass refusal tests remain regression coverage.

This is not a customer/public release, full S0-S10 acceptance or completion of
the overall TODO goal. True customer business signing remains a separate gate.
