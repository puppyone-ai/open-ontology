# Handoff

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a208`. Fixed source `a661dcdf72ff91561ae6b7cb79916e582884d08a`, parent `84c03d01ce35728924100e42ca142c0ef650ac71`. Query CLI and executor blobs are unchanged from `4f3f329aa0dabb3f848e6040fdc5379df921c2f9`. Author evidence a198 `33e5723e107407d267ab115ca86185145e4dcb6d` and a206 `90dfac17534983e3f3f93d5f437806193c24095b`. Prior catalog review a200 `a587031c719a484830d78e493605f48e2056cae4`.

The approval is the compiled `query-pg` composition on the fresh `oo_a208` fixture, plus the a206 three-path alignment actually run with `OO_A190_LIVE=1`. Executor a205 stays **REQUEST_CHANGES** (`cancel_reserved` / `SessionBusy`, and the pre-guard `oo_r_` bind). Do not treat this directory as an executor approval.

`AGENTS.md` is absent. No product, manifest, lock, or ledger edit. No natural-language, authentication, production-WAL, full-harness, parent-acceptance, or release claim.

## Read first

- `FINDINGS.md` for the executor lines, the exit table, the audit sample, and the digest-scope comment.
- `live/cli-oracle.txt` lines 38–97. Lines 1–37 are the attempt 003 prefix. The passing copy ends `done=ok`.
- `live/oracle.txt` for `guard_bound_role=clerk`, `guard_audit_delta=0`, `role_reuse=refused`, `fresh_other_role=auditor`, `fresh_read_audit=1`.
- `linux/proof/docker-run.log` and `linux/proof/phases/` for the decisive exits. `proof.exit` is 0.
- `linux/smoke/smoke.exit` is 0, written before the overlay existed.
- `linux/proof/attempts/001` through `005` for the failed runs.
- `source/blobs/` for the bytes that were reviewed. `source/COMMIT` is `a661dcdf72ff91561ae6b7cb79916e582884d08a`.

## Decisive exits

Proof container `56575ddbdb1220f03c2343f4fdb13921447a8f855e380f4b7fc6b3b5ef413efd`. `docker-run.exit` 0, `launch.exit` 0, `proof.exit` 0. `oom=false`, `privileged=false`, `capdrop=["ALL"]`.

`a190-live.exit` 0 in 0.77s. `a190-oracle.exit` 0 (`keys=present`). `a208-gate.exit` 0 and `a208-gate-message.exit` 0. `a208-live.exit` 0 in 11.48s. `a208-oracle.exit` 0 (`done=ok`). `clippy.exit` 0. `lock-hash.exit` 0, lock `9608f88b7dbe46674e064f005280768cf9c383cce18827a57a8241056fa03f08`. `secret-scan.exit` 0.

Smoke container `2809f34a928eebff1596eed83f6a84cff399986d8e11aeb44512c69f82b97a4e`. PostgreSQL `17.10 (Debian 17.10-1.pgdg12+1)`, image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Rust `1.94.0 (4a4ef493e 2026-03-02)`, image `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`.

## Do next

Leave the executor reservation and the pre-guard bind for a change that is about the executor. The CLI review is not that change.

If the `StaleGeneration.current` comment is edited, keep the SQL predicate `source` and `config_digest`. The live fixture already distinguishes another digest's generation 21 from this digest's generation 6.

A login that cannot read `_oo.physical_mappings` or execute `lookup_physical_mapping` was not the subject of this run. Those grants were applied by the fixture.

## Review

Grok 4.7 in this session was the only coder, tester, and reviewer. No subagent and no worktree. One Rust builder for this task, one private PostgreSQL. Failed proof attempts 001–005 stay. The decisive proof exit is 0.

## State left behind

`cleanup.txt` records removal of this task's containers, volumes, network, and `/tmp/oo-a208-eef831aa-dba4-4af2-a78b-541bc9e0a208`. Pinned images stay. `docker prune` was not used. Other tasks' containers stay.

`SHA256SUMS` lists the other files in this directory. `SHA256SUMS.sha256` is the digest of that list. Both were written by `tools/sha256.rs`.
