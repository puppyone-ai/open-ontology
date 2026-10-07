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
| G3 or G4 `warn`, not blocking, same binding | `admissible` | test scope only |
| G8 recompile matches, samples `omitted`, publication `not_claimed` | `admissible` | test scope only |

`component_authority` for G8 reruns `compile_role_pack` on the stored inputs. The caller’s status is not trusted. The recorded evidence is `role_pack_samples_omitted`. `pii_database_scan` is `not_performed`. That pass is not a production certificate.

Production admissibility requires a production issuer. This slice does not define one, so every test, fixture, and component row is `not_production_admissible`. Production also requires the five external names above. Missing or test-issuer rows block production. The activation transaction still evaluates that list. It does not return early through a waive flag.

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
