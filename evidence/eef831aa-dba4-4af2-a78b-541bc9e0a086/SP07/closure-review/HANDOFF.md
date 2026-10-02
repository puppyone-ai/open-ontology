# Handoff — a086 structure-closure fixed review

Stop here. The coordinator commits the received records. This session does not commit, push, or release.

## Decision

Approve findings 3 and 4 on `eebc0ea3b835655fdb3ab088e6f7351d0d316a43` (tree `eebbcb4a8ff44d50c4b541d5646796a82a234210`, parent `79587e4bcc168b1cb2bfeee2690f62225ddb7dcd`):

- `effective-fk-names-rejected-column`
- `duplicate-key-member-accepted`

SP07-W01, whole SP07, the vertical, security, runsc, customer delivery, protected CI, and release stay unsigned. Findings 1, 2, and 5 stay open. Numeric scale, PostgreSQL 63-byte mapping, unverified foreign-key semantics, real PostgreSQL, RBAC, profile, LLM, publication, and query stay unsigned. The connectors interface stays unaccepted until an independent a083 rebuild and review. a073 in the a081 report is stale. The live owner is a083.

## Evidence

- `review.md`
- `findings.json`
- `governance-notes.txt`
- `probe/a086_probe_tests.rs`
- `msrv/attempt-1` through `msrv/attempt-6`

Attempt-1 smoke fetch, test, and clippy exited 0 (lib 27, integration 4). `cli.exit` 1 and `probe.exit` 101 are preserved checker and probe defects. Attempt-2 was deferred before docker run. Attempts 3, 4, and 5 are preserved probe-input failures. Attempt-6 is the deciding probe: docker exit 0, 118 cases, 0 failures, `summary.json` ok true.

## Cleanup

Removed at 2026-10-02T10:55:43Z through 10:55:45Z, after a label recheck. Five exited containers, network `15111804e05b9d49c40bebe3cf2c94f32a4a51fb9553ee376b0d7f35dcf31e48`, and the two `oo-a086-e2364472-4959-4f54-b815-2ac993ff51ff` volumes. Command exits 0. Owned resources remaining: 0. The image stayed. No prune. Historical exit files stayed as written. IDs are in `findings.json` and `cleanup.txt`.
