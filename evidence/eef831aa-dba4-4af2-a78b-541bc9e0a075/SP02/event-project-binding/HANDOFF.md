# Handoff

a075 event project binding. Parent `304fcf9f116bbbabe0f91a2caca5df5949ef476c` (`parent.txt`). Jobs schema baseline `70a4e72c3ea698038e0bdd38df29e38c01dfd24d` is an ancestor. This file is inside the task commit and does not name the task commit. Read that SHA with `git rev-parse HEAD` after the commit. No push, pull request, or release.

Owned paths:

- `crates/oo-jobs/migrations/0002_event_project_binding_expand.sql`
- `crates/oo-jobs/tests/event_project_binding_contract.rs`
- `crates/oo-jobs/tests/event_project_fixture.sh`
- `crates/oo-jobs/tests/fixtures/event_project/`
- `contracts/jobs/event-project-binding/`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a075/`

`jobs-event-project/1` adds non-deferrable `UNIQUE (id, project_id)` and `task_events (task_id, project_id)` referencing that unique key. A preexisting mismatch raises `OO004` and rolls back. Release checksum `4ede04e8f848eff002da7e3a12628ac731c7bca02ba345daa6d036790e894fee`. `0000`, `0001`, and `jobs-control-v1.json` keep their existing pins. Details, SQLSTATEs, and the run records are in `REPORT.md`.

The completed item is this event-binding subitem. Full SP02, the jobs driver, a 100-worker run, backup and restore, and the vertical flow stay pending. Claim and compare-and-swap, the 30 second lease, the 10 second heartbeat, bootstrap checksum trust, an adopted definer, and the target fence stay pending. Event contents remain caller-supplied.

a073 owns the main root dependency set. This commit does not edit a root manifest or another module. The a075 containers, networks, volumes, and temporary password directory were removed after the evidence files were saved. The a072 fence container was left in place.
