# ADR-NNN: Title

| Field | Value |
|---|---|
| Status | Proposed |
| Date | YYYY-MM-DD |
| Supersedes | none, or ADR-00N |
| Superseded by | none |

Copy this file into the draft you are reviewing. Do not pre-create the next number as an accepted decision.

## Numbering

Continue the series in [docs/02-technology-stack.md](../docs/02-technology-stack.md). That series is ADR-001 through ADR-016. The next free number is whatever that file does not already use on the day you write the draft. Re-read the file before numbering. Do not start a second series (no `GOV-ADR`, no `ADR-SEC`).

Landing the text into `docs/02` and into the Spec is a documentation change those files' owners have to accept. A draft under `governance/` does not amend the Spec. [Spec §19.3](../specs/open-ontology-v0.1-spec.md) requires a requirement or acceptance change to update the requirement, the acceptance case, the phase, the source document, compatibility, and migration. Historical identifiers are not reused.

Proposed becomes Accepted only with the prototype evidence the decision needs. Accepted becomes Implemented only with code and automated tests. This template does not change either status.

## Background

What is hard, which document it comes from, and which options were rejected. Cite `REQ-`, `DEC-`, `P`, or `ADR-` identifiers that already exist.

## Decision

The rule a later implementer can follow without guessing. If this relaxes a security invariant, stop. The decision is not ready.

## Consequences

What gets easier, what gets harder, and which component now owns the new failure.

## Switch condition

When this decision is revisited, and what evidence would cause that. "Never" is allowed when the current ADR series says so. Say it explicitly.

## Requirements and acceptance

| Kind | Identifier | Scope |
|---|---|---|
| Requirement | `REQ-…` | |
| Acceptance | `AT-…` | phase slice, not the whole case unless the whole case was run |
| Prototype | `P…` or none | `validated` / `rejected` / `blocked` / not run |

## Compatibility and migration

- Protocol or file format version:
- Expand / migrate / contract, checksum, single-writer lock:
- Old binary window:
- Rollback or forward fix:
- Source database write: none. If the draft needs a source write, it is out of policy.

## Security and supply chain

- Trust boundary touched:
- New dependency, native code, `unsafe`, `build.rs`, or proc-macro: none, or a record under [the supply-chain policy](../supply-chain/policy.md)
- Secrets, sandbox, session release:
- Data: synthetic only

## Evidence

Commands, exit codes, and paths. An empty section means the decision is still Proposed for lack of evidence, not Accepted.

## Review

| Role | Person | Conclusion | Date |
|---|---|---|---|
| Component owner | unassigned | | |
| Security owner, if the decision touches a trust boundary, SQL, a secret, the network, native code, or release | unassigned | | |
| Supply-chain owner, if a dependency is added | unassigned | | |

The author does not fill the reviewer column with their own name and call it done.
