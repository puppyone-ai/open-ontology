## What this changes

<!-- One reviewable behavior. Say what a user or operator can observe. -->

## Existing identifiers

<!-- Product work cites both an existing REQ- id and an existing AT- id,
     plus dependencies, the target crate, the expected files, and the
     failure behavior. A partial AT slice names its phase, for example
     AT-SEC-05 at SP00-W06. Do not invent a new product numbering scheme.
     Governance text with no product id: write "governance-only: no product id". -->

## Contract

<!-- Inputs, outputs, identity, permissions, version, timeout, cancellation,
     idempotency, and failure behavior. Write "unchanged: <link>" if an
     approved contract already covers this. -->

## Evidence

<!-- Command, exit code, environment, and evidence path.
     Docs-only: name the document checks you actually ran.
     Do not write "should pass". -->

## Migration and schema compatibility

- [ ] No migration or schema change.
- [ ] Expand, then migrate, then contract. Checksum and single-writer lock are described. The old binary window is stated. Rollback covers data and grants, not only the image.

## Async bounds and cancellation

- [ ] No new stream, channel, queue, or batch.
- [ ] Limits are stated. Drop, timeout, or abort is not the proof that PostgreSQL, a container, or a token was released.

## FFI, native code, and licenses

- [ ] No new `unsafe`, native code, `build.rs`, proc-macro, or dependency.
- [ ] The supply-chain record is attached. Rust and security / supply-chain reviewers are named. The build has no production secret.

## Source writes

- [ ] This diff does not add a source-database write or source DDL.

## PostgreSQL grants and row-level security

- [ ] No authorization change.
- [ ] Read authority stays in PostgreSQL roles, column grants, and row-level security. New columns stay hidden until published.

## Sandbox credentials

- [ ] No model credential, source secret, or session token is added to an image, environment, pack, or workspace.

## Role or policy change

- [ ] No role or policy change.
- [ ] Shrink ends affected sessions, revokes tokens, and unloads the old pack. A failed publish stays closed.

## Session release

- [ ] No session-lifecycle change.
- [ ] S1–S11 release is covered, including crash recovery by the reaper.

## Holdout and customer data

- [ ] The diff and the evidence contain no customer sample, holdout item, gold row, internal domain, or secret.

## Review

- [ ] The reviewer is not the author of a security-relevant change.
- [ ] Reviewer roles required by Spec §19 are named in the request. They are roles, not a claim that GitHub CODEOWNERS is already enforcing them.

## Release blockers touched

<!-- Leave blank, or name one: private security contact, conduct contact,
     release contact, branch protection, SBOM/signature for an M1 artifact. -->
