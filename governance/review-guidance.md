# Review guidance

Reviewers use this with [Spec §19](../specs/open-ontology-v0.1-spec.md) and [Plan §2](../specs/open-ontology-v0.1-plan.md). The author of a security change is not the reviewer. A commented CODEOWNERS file does not count as the review. See [ownership-map.md](ownership-map.md).

Record the conclusion, the date, and the open limits in the task evidence. "Looks good" without a command or a scope is not a conclusion. A partial slice does not mark the parent acceptance case passed.

## Every pull request

- The request cites existing `REQ-`, `AT-`, `SP`, `P`, `G`, `I-`, `DEC-`, or `ADR-` identifiers, or it is a governance typo with no product id.
- One reviewable behavior. A security change is not folded into an unrelated refactor.
- Breaking protocol, wider permissions, migrations, new dependencies, and runtime privileges are marked in the request.
- The evidence names the command, the exit code, and the commit. A docs-only change names the document checks it actually ran.
- Public artifacts contain no customer data and no secrets.
- Mocked success is not acceptance of a security boundary.

## Contract and failure behavior

- Inputs, outputs, identity, permissions, version, timeout, cancellation, and idempotency are stated.
- The failure and the degraded mode are implemented, not only the success path.
- An expected rejection is the specified error and has no extra side effect. A 500 is not that test.
- There is a legal control that shows the fixture environment works.

## Migrations and compatibility

- Expand, then migrate, then contract, with a checksum and a single-writer lock.
- The old binary's compatibility window is stated.
- Rollback accounts for data and grants. Replacing an image is not a rollback plan when the schema moved.
- Two databases are not described as one transaction.

## Async bounds and cancellation

- New streams, channels, queues, and batches have row, byte, concurrency, and wait limits.
- `Drop`, a proxy timeout, abort, or a cancel message is not offered as proof that PostgreSQL, a container, or a token is released.
- The confirmed release (backend gone, rollback done, connection not reused, container exited, token revoked) is what the evidence shows.

## FFI, native code, and dependencies

- `unsafe` is limited to the wrapper that needs it, with the invariant written down.
- `build.rs` and proc-macros are treated as build-time execution. The build has no production secret.
- A new default dependency has an audit record: official source, version, publication date, MSRV, features, `default_features_disabled`, license expression and LICENSE evidence, native transitive objects, digest, and an advisory disposition. The seven-day rule holds only with an appointed named owner and regression evidence. `OWNER_PENDING` means that exception is not valid.
- License expressions use SPDX identifiers, `AND`, `OR`, and parentheses. A prohibited token, strong copyleft, or an unknown `LicenseRef` fails the default record even inside `OR`. Weak copyleft needs a named review with obligations, evidence, and scope. `true` is not a review. One component's review does not cover another.
- AGPL, BSL, ELv2, and SSPL are absent from the default artifact. A ledger comment is not an exception for them. Absence or a pending advisory does not clear the record.
- A known exploitable high-severity issue blocks the default artifact. `governance/check_governance.py` checks the record shape. It does not run `cargo audit`, `cargo deny`, or an image or native scan.

## Source writes, grants, and the sandbox

- The diff does not add a source-database write or source DDL. Permission checks that find a writable source account fail closed.
- Read authority stays in PostgreSQL roles, column grants, and row-level security. New columns and derived objects stay hidden until published.
- The sandbox image, environment, and workspace gain no model key, source secret, or session token.
- Gateway still does not link the connector, the loader, or management DDL.

## Role changes and session release

- A policy shrink ends the affected sessions, revokes tokens, and unloads the old pack. The session does not keep the old grant with the old semantic version.
- Publish failure stays closed until a safe policy is restored and the replica barrier has completed.
- Session end covers S1–S11. The five-second revocation and sixty-second destruction targets are measured when the runtime exists, not assumed from process exit.
- The reaper can finish a release after the entry process dies.

## Holdout and ontology meaning

- Public diffs and public CI logs contain no customer sample, holdout item, gold row, or production query.
- Holdout results, when any exist, are aggregate. They were not used to edit individual items in the same change.
- Certified metrics keep an independent control total and an owner. A model draft is not a published fact.
- Human edits, historical keys, and validity windows survive the change.

## Phase exit

A phase exit walks the work-item list. Missing work stays `planned` or `blocked`. One green slice does not close the phase. Customer acceptance is a customer signature on a version, an expected result, and an actual result. A screenshot is not source-write evidence and it is not permission evidence.
