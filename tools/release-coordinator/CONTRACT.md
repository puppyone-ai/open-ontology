# oo.release-coordinator/v1

SP08-W04 / E53-D1/1 component. One project. One PostgreSQL schema `oo_release_control`. This file was frozen before the store, the migration, and the tests.

This crate does not accept SP08, a public deployment, customer ownership, a standby, or a production-ready certificate. `full_sp08_accepted`, `public_deployment_claimed`, `customer_ownership_claimed`, `standby_configured`, and `production_ready_certificate` are false on every receipt and in the deployment row. `test_mode_cannot_produce_production_certificate` is true on every receipt. Test-scope activation sets `certificate_class` to `test_scope_not_production`. Every other outcome sets `certificate_class` to `not_issued`.

The crate calls `oo_builder::compile_role_pack` on the caller’s effective OOS document and visibility snapshot. It does not implement a second pack compiler, catalog compiler, policy compiler, or metric compiler. Policy id, policy version, knowledge id, knowledge version, catalog version, and role come from that compiler’s `manifest.json`. Caller `expected_policy`, `expected_knowledge`, and `expected_catalog_version` must equal the manifest or stage writes nothing.

## What is refused

There is no `force`, `waive`, `all_accepted`, or `allaccepted` field. A top-level command object that contains one of those keys is malformed and writes nothing. `product_mode_can_activate: true` is malformed and writes nothing. This slice has no production issuer. Fixture authority class is only `bounded_local_fixture`, and its statement fixes `product_mode_can_activate` and `production_ready_certificate` to false.

Credential, integrity, authenticated owner, customer, and runsc receipts are not created by this crate. A caller may record an explicit `test_issuer` row for those names. That row does not set `credential_verified`, `integrity_verified`, `owner_authenticated`, `customer_gate_verified`, or `runsc_verified`. Those columns stay false. Session termination is not performed. A checkpoint with `sessions_terminated: true` is stored as `fabricated_session` and is not a fresh barrier.

G10’s documented owner exception is not implemented. An authenticated owner is not invented to satisfy it.

## States

Spec sequence `draft → validating → candidate_built → gates_passed → applying → replica_ready → active`, with `failed` / `recovering` on failure. This slice does not persist a partial draft. Stage either commits `candidate_built` or writes nothing.

| State | Meaning |
|---|---|
| `candidate_built` | Seal committed. Pointer unchanged. |
| `gates_passed` | Test-scope plan admissible. Pointer unchanged. |
| `applying` | Durable intent after the first activation transaction. Pointer unchanged. |
| `active` | Pointer names this candidate. Same transaction as the activation audit. |
| `inactive` | Was active, or was rolled back. Seal and pack hashes remain. |
| `failed` | Applying did not publish, or a later check failed. Pointer unchanged. |
| `aborted` | Terminal. Not active. |
| `retired` | Terminal. Not active. |

`replica_ready` is an audit event in the same transaction as `activated`. It is not a durable state that can be read without the new pointer. `recover` writes audit event `recovering` and state `failed` in one transaction.

A crash after `applying` and before the activation transaction leaves the previous pointer in place. `recover` moves that candidate to `failed` without changing the pointer. Partial preparation is never the active version.

## Seal

`stage` canonicalizes `effective_oos` and `visibility`, compiles the pack, and checks every manifest path with `normalize_pack_path`. Paths, sha256, and byte lengths from `manifest.json` are stored. File text is not stored. The compiler file bytes are hashed again and must equal the manifest sha256.

`source_hash` is SHA-256 of the canonical seal: protocol `oo.release-coordinator/seal/v1`, project, candidate, version, base version, both input hashes, pack digest, manifest sha256, policy, knowledge, ontology version, catalog version, role, and compiler.

The same candidate and the same source hash replay. A new source hash, while the state is `candidate_built`, `gates_passed`, or `failed`, replaces the seal, deletes gate rows and checkpoints, and returns the state to `candidate_built`. The same new hash does not keep old gates. `applying`, `active`, `inactive`, `aborted`, and `retired` reject a new source hash with no write.

`base_version` is the active pointer version at stage time (`null` when the pointer is empty). A mismatch writes nothing.

## Gates

Applicable publication gates are `G1`, `G2`, `G3`, `G4`, `G5a`, `G5b`, `G6`, `G7`, `G8`, `G9`, `G10`, `G11`, `G12`, `G13`.

| Intake | Stored disposition | Can pass |
|---|---|---|
| no row | `missing` | no |
| `status: not_run` | `not_run` | no |
| source hash differs from the seal | `stale` | no |
| fixture authority hash differs, or fixture scope is not `test_scope` | `foreign_receipt` | no |
| `component_authority` for a gate other than G8, or G8 recompile mismatch | `component_uncovered` | no |
| two admissible certificates for one gate | `conflict` | no |
| `pass`, not blocking, source matches, test issuer or covered fixture, scope `test_scope` | `admissible` | test scope only |
| G3 or G4 `warn`, not blocking, same binding, `test_issuer` | `admissible` | test scope only |
| G3 or G4 `warn`, not blocking, covered `fixture_authority` target measurement | `not_run` | existing store warning handling only; not production |
| G8 recompile matches, samples `omitted`, publication `not_claimed` | `admissible` | test scope only |

`component_authority` for G8 reruns `compile_role_pack` on the stored inputs. The caller’s status is not trusted. The recorded evidence is `role_pack_samples_omitted`. `pii_database_scan` is `not_performed`. That pass is not a production certificate.

Production admissibility requires a production issuer. This slice does not define one, so every test, fixture, and component row is `not_production_admissible`. Production also requires the five external names above. Missing or test-issuer rows block production. The activation transaction still evaluates that list. It does not return early through a waive flag.

### Business receipt trust boundary

`BUSINESS-NATIVE.md` adds the explicitly validated native C1 published ordinary-
shadow path to the same test-scope issuer resolver. It retains column grants,
binds the actual publication/current vector and refuses overlays/views or
unsupported codecs. No source-row courier is used for native data binding.

The generic test-issuer admission table above is not a business-evidence proof.
Applicable G5a/G5b/G6/G10/G11 claims (and G12 lifecycle requirements) reject a
generic nonblocking pass at intake. Status/plan/activation/CAS rechecks also apply
the stored candidate's current preconditions, after maintenance overlays, so
historical generic pass rows or stale `not_applicable` rows cannot bypass them.
This closes the direct-plan path without changing production admissibility.

`BUSINESS-EVIDENCE.md` specifies the validation-only candidate/role/data-vector interface.
The additive `BUSINESS-POSITIVE.md` path now consumes actual, separately
authenticated test-scope execution/oracle/eval/review records and can satisfy
G5a/G5b/G6/G10/G11. It reuses the existing plan/CAS and production flags.
It does not reinterpret the generic `test_issuer` admission table.

The historical fallback checker specifies the candidate/role/data-vector/issuer interface
and the executable validation-only receipt contract. A perfectly bound body is
still refused as `business_issuer_unavailable`: the current store has no separate
authenticated execution/review issuer and immutable receipt resolver. No accepted
JSON flag, content hash, test label or self-declared reviewer replaces that trust
dependency. That checker remains non-admitting; only the private installed
policy plus current immutable database evidence enters the positive resolver.

### Library G4 target observations

`library_semantic_observations` measures local target code coverage and the
structure of declared reviews, not dictionary-label truth or authenticated owner
approval. It does not access a source database, publish, certify a metric, or
issue a production certificate. Neither a label nor a declared reviewer is
independent evidence that a business meaning is correct.

The server-generated detail has prefix `independent_target_enum_observations;`
followed by JSON contract `oo.release-coordinator/enum-observations/v1`. It binds:

- The canonical SHA-256 of the current effective document, plus the canonical
  SHA-256 of each entire enum definition (including codes, labels, classes,
  owner, reviewer, review state, source, evidence, and validity fields).
- Every logical enum to its physical schema/table/column. Binding cardinalities
  and logical column identities must be unambiguous; no enum is silently skipped.
  An approved enum without a target mapping is `fail`, blocking.
- The target endpoint, database, observer login, PostgreSQL snapshot and
  transaction timestamp. Counts run under TLS in one read-only repeatable-read
  transaction, with the existing active-RLS rejection. The transaction rolls back.

Definitions use the existing OOS enum parser/validator. An approved code needs a
non-reserved, declared class, a nonblank label, a declared nonblank enum owner,
and `review.state=approved` with a nonblank `review.by`. The owner can be a group
and the reviewer an individual; equality is not identity authentication.
Missing review/owner/label evidence is blocking `not_run`, not an inferred
approval. Invalid definitions, duplicate codes and reserved approved classes
are blocking `fail`. All other statuses, including deprecated and superseded,
are excluded from approved coverage even if they have labels or old reviews.
Temporal definitions remain blocking `not_run: temporal_scope_unbound`: this
observer has no row-time/as-of binding and must not union different eras.

For every mapped, structurally valid enum, a real full target-column aggregate
measures `non_null_rows`, `unlisted_rows`, `approved_rows`, and
`listed_unapproved_rows`. Codes are parameterized, compared as exact PostgreSQL
text under the `C` collation, and never interpolated into SQL. NULL is excluded;
empty strings are not NULL. Caller `coverage`, `unexplained_values`, and
`evidence.rows` are retained in the definition binding but never substituted for
measured counts.

Approved row coverage is the exact fraction `approved_rows/non_null_rows`.
The 95/100 technical threshold is evaluated **per enum**, without floating
point or averaging away another enum's gaps. Below-threshold coverage is a
measured warning with `review_needed=true`, not a coverage pass; this preserves
the documented warning/review treatment and does not prohibit unrelated raw
statistics. Remaining unapproved rows require review even above the threshold.
Empty or all-NULL columns have undefined coverage, not 100%. With approved
codes they yield blocking `not_run`. Only when a successful target query proves
zero non-NULL rows and there are zero approved codes, the result is nonblocking
`warn: no_approved_semantics_empty_field`, with undefined/null coverage and
`review_needed=true`. Undetermined codes do not count as approved, even if they
carry labels or reviews. Missing approved review/owner evidence is rejected
before counting and never receives this exception.
Missing mappings without approved codes are also blocking `not_run`;
query/permission failures abort observation rather than inventing evidence.
An empty effective enum list retains `measure_g4`'s nonblocking empty warning.

Measured results are **at most nonblocking `warn`**, even at 100% coverage.
The report always states `business_label_accuracy=not_measured`,
`business_owner_signed=false`, `owner_authenticated=false`, and
`production_certification=not_issued`. The existing `decide.rs` mapping of a
covered fixture G4 warning to `not_run` and `production_admissible=false` is
unchanged. Technical directory/candidate deployment and final business
acceptance are distinct from a production certificate.

During activation, a blocking non-admissible measured gate, a non-admissible
G8/G7, or a refused gate-record operation returns `refused`, `reason=gate_failed`,
exit code 2, with the original gate receipt and gate id/detail retained. Recording
a gate successfully is not a successful activation. The standalone `gate-record`
command continues to return 0 when recording succeeds.

This output contract is not an input for accepting owner or accuracy claims.
Business acceptance still needs separate, independently verified meaning and
owner evidence bound to the definition, data/time scope, and relevant metric.
95% code coverage cannot establish completeness for a certified metric or
justify dropping unknown financial amounts. G5 and other business gates remain
unchanged; this observer neither satisfies nor waives them.

Regression tests are included from `tests/observations/enum_observations.rs`.
Run unit tests with `cargo test --locked -j 1 --lib target::enum_observation_tests`.
The explicitly ignored local PG test requires `OO_G4_TEST_CONFIG` containing
host, port, database, user,
`password_file`, and `ca_file`; it accepts only loopback, a dedicated
`g4_observer_test_*` database/login, and a non-superuser/non-bypass-RLS login.
Run it with `cargo test --locked -j 1 --lib local_pg_enum_observer -- --ignored
--nocapture`. It creates/removes only its fixture schema and never invokes
release/install/activation APIs.

## Fence, replica, CAS

A fresh fence has the candidate source hash, barrier `closed`, a token epoch, `sessions_terminated` false, and a test issuer or the installed fixture authority. A fresh replica has the same source hash, the same epoch, and barrier `replayed`. Any other barrier is not fresh. Two fresh fences, or two fresh replicas, are a conflict.

`plan` for `test_scope` moves `candidate_built` or `failed` to `gates_passed` only when every applicable gate is admissible and both checkpoints are fresh and the expected version equals the pointer and the sealed base. Otherwise the state is left unmoved, except that a stale `gates_passed` plan returns to `candidate_built`. A production plan never moves the state to `gates_passed`.

`activate` for production does not enter `applying` when the production list is non-empty. For test scope:

1. Transaction one, when the test plan is still admissible: state `gates_passed` becomes `applying`. The pointer is not updated. Repeating this step is idempotent.
2. Transaction two locks the pointer, rechecks gates, fence, replica, and the expected version, then inserts audit events `replica_ready` and `activated`, moves the pointer, sets this candidate `active`, and sets the previous active candidate `inactive`. One commit. A failed check sets this candidate `failed` and does not move the pointer.

A second activate of the same active candidate returns the same audit id.

Two candidates that both expect the same base: the activation transaction updates the pointer only when `version IS NOT DISTINCT FROM` the expected base. The loser becomes `failed`. The winner stays active.

## Recover

`recover` from `applying` sets `failed` and does not move the pointer. `recover` from `failed` replays. `abort` and `retire` from a non-active, non-inactive state set `aborted` or `retired`. They refuse `active`. `rollback` requires the candidate to be `active` and the expected version to be the pointer. It moves the pointer to `base_version` (or to empty when the base is null) in one transaction, leaves both seals in place, and does not compile again. The rolled-back candidate becomes `inactive`. The restored candidate becomes `active` again. A repeat of that rollback replays.

Pack paths stay the normalized compiler paths. Rollback does not rewrite them.

## API

```text
release-coordinator install --config CONFIG
release-coordinator stage --config CONFIG
release-coordinator status --config CONFIG
release-coordinator gate-record --config CONFIG
release-coordinator plan --config CONFIG
release-coordinator activate --config CONFIG
release-coordinator recover --config CONFIG
```

The request is one JSON object on stdin, at most 2_097_152 bytes. `CONFIG` names host, port, database, user, `password_file`, and `ca_file`. The password and the CA are not argv. Stdin and stdout do not contain them.

| Condition | Exit | Stdout |
|---|---|---|
| malformed argv, JSON, unknown field, waiver key, `product_mode_can_activate: true` | 1 | empty |
| refused business rule | 2 | one report |
| committed or replayed | 0 | one report |

User for these commands is `oo_release_admin`. The migration grants `oo_release_reader` select on `publication_status` only. That view exposes the pointer and the standing false claims. It does not grant update on the pointer.

Install executes `migrations/oo_release_control_v1.sql` once, in one transaction, and stores that file’s SHA-256. The same hash, project, and authority statement replay. A different hash or statement is `install_drift` and is not repaired.

TLS is required. Certificate verification uses the caller CA. There is no `NoTls` path.

## Dependencies

Direct pins already in the repository root lock, plus path package `oo-builder` at `../../crates/oo-builder`. No new crate, no version bump, no root manifest edit, no root lock edit, no `cargo update`. `rust-version` is 1.94.0. Safe Rust only.
