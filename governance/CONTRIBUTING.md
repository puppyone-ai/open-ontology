# Contributing

Restoration note (2026-11-02): the original implementation-status statements
below are dated October 1, not a claim that the current source tree is empty.
Contribution and security rules remain applicable; current capability claims
require their own evidence.

> 2026-10-01。贡献规则以 [Spec](../specs/open-ontology-v0.1-spec.md) 和 [Plan](../specs/open-ontology-v0.1-plan.md) 为准。本文不另起一套编号。

Open Ontology is a design and an implementation plan. The product those documents describe is not built. A contribution has to follow the existing contract. It does not redesign the project in a pull request.

Product behavior is [OO-SPEC-001](../specs/open-ontology-v0.1-spec.md). Order of work and evidence are [OO-PLAN-001](../specs/open-ontology-v0.1-plan.md). Design sources are the files under [docs/](../docs/README.md). When a design example and a security rule disagree, follow the stricter security rule and record the conflict. Do not edit the Spec or the Plan so that a patch looks compliant.

## Identifiers

Use the identifiers that already exist:

| Family | Meaning |
|---|---|
| `REQ-<域>-NN` | Implementation requirement |
| `I-SRC` / `I-PL` / `I-DB` / `I-GW` / `I-RT` | Safety and correctness invariants |
| `DEC-NN` | Implementation decision, including prototype gates |
| `SP00`–`SP15`, `SPxx-Wyy` | Phase and work item |
| `SPxx-Wyy-a` / `SPxx-Wyy-b` | Split of an existing work item. The parent stays. |
| `AT-<域>-NN` | Acceptance case. Record the phase scope, such as `AT-SEC-05` at the SP00 foundation slice. |
| `G1`–`G13`, including `G5a` and `G5b` | Ontology publish gates |
| `P1`–`P16` | Risk prototypes |
| `ADR-NNN` | Architecture decision. The series lives in [docs/02](../docs/02-technology-stack.md) and currently runs ADR-001 through ADR-016. |

Do not invent a second product numbering scheme. An issue that cannot point at one of these, and is not a typo in a governance file, is not specified work.

## What you can run

Document checks that exist today:

```bash
python3 specs/scripts/check_spec.py --self-test
python3 specs/scripts/check_plan.py --self-test
python3 governance/check_governance.py
git diff --check
```

`python3 governance/check_governance.py` reads the governance files, including files git does not track yet. It writes nothing unless `--write-evidence` is set, and that flag writes only the correction evidence directory named in the script. `git diff --check` does not read untracked files. A clean exit from it is not a content check of those files. If `yamllint` is missing, that check is skipped. Skipped is not passed. Ruby `YAML.safe_load` is the parse that still has to succeed.

Cargo, xtask, Compose, `./oo up`, `cargo audit`, `cargo deny`, and product tests are specified later. They are listed as planned in [README.md](../README.md). A missing command is not a pass. The governance checker is not those scans.

## How a change is made

The Plan's order is: source and risk, contract and an observable failure, a small real slice, negative cases and recovery, integration, independent review, evidence. For one work item that becomes:

1. **Contract.** Fix inputs, outputs, identity, permissions, version, timeout, cancellation, idempotency, and failure behavior. Add legal and illegal fixtures. If an approved contract already covers the change, cite it.
2. **Failing check.** For protocol, permission, precision, concurrency, and recovery, add a check that fails when the behavior is wrong. A legal control shows the environment works. An expected rejection must be the specified error, with no unauthorized side effect. HTTP 500 is not that proof.
3. **Smallest real path.** Include cancellation, timeout, audit, and the recovery the contract needs. Do not merge a data path that has no protection yet.
4. **Boundaries.** Dirty values, reorder, concurrency, disconnect, full disk, kill, and retry. A security fix ships with the capability it protects.
5. **Integration.** Run the applicable acceptance cases on real dependencies. Save evidence.
6. **Handoff.** State the scope, the commands, the exit codes, the limits, and what the next task can use.

Risk prototypes P1–P16 are real spikes on real dependencies. A prototype result is `validated`, `rejected`, or `blocked`. It does not mark the product done. If a prototype fails, the dependent default capability stays frozen.

One pull request has one reviewable behavior. Mark, in the request itself, any breaking protocol change, permission expansion, migration, new dependency, native or `unsafe` code, or runtime-privilege change.

Branch names in the Plan, when a branch is needed, look like `codex/spxx-wyy-description`. Do not commit secrets, customer data, or files under someone else's write ownership. Do not deploy, publish, or access a customer system from a contribution task unless that task explicitly authorizes it.

## Rules that stay in force

### Migrations and schema compatibility

A migration uses expand, then migrate, then contract. It has a checksum and a single-writer lock. State the window in which the old binary still runs. Rolling back is not "put the old image back" when rows or grants have changed. An incompatible schema change needs a tested restore or a forward fix. The source database is never the place that fix is written.

### Async work, bounded memory, and cancellation

Streams, channels, and queues have row, byte, concurrency, and wait limits. The initial batch cap in the Spec is 10,000 rows or 16 MiB, whichever comes first. Do not `collect()` a whole table, and do not add an unbounded channel or an unbounded server-sent-event backlog, as the default.

Propagate stop with a cancellation token, and track tasks. `Drop`, a Tower timeout, abort, or a cancel request does not prove that a PostgreSQL backend, a portal, a container, a token, or a multipart upload is gone. Show the confirmed release.

### FFI, native code, and licenses

`unsafe` stays in the FFI wrapper that needs it, with the safety assumption and a reviewer recorded. `build.rs` and proc-macros are code that runs at build time. They build without production secrets. New native code, new dependencies, and hooks follow [supply-chain/policy.md](../supply-chain/policy.md) and need the Rust reviewer plus the security and supply-chain reviewers. A note on a ledger does not admit a prohibited license or a known exploitable vulnerability.

### No source writes

The platform does not write source business data and does not run source DDL. CDC setup objects (publication, slot, replica identity) belong on the DBA checklist. The connector does not create them by itself. A permission check that finds a writable source account stops that source.

### PostgreSQL grants and row-level security

The authority for "what this user may read" is the PostgreSQL role, column grants, and row-level security. An application-level filter can reject early and explain the error. It is not the boundary. Gateway does not link the connector or the loader. New columns stay invisible until they are classified and published. `SELECT *` is expanded to the visible columns. That expansion is the only permitted rewrite. R9 still judges an unfiltered `SELECT *` on a large table by the original star. Expansion does not bypass that rejection.

### No model credentials in the sandbox

The sandbox has no secret. The egress proxy holds the session token and the model key and injects them on the fixed upstream paths. The model key is not stored in shared Valkey. A session token, if it must be stored for the proxy, is not readable by the Gateway account. Do not put a model key, a source password, or a session token in an image, an environment file, a knowledge pack, or `/workspace`.

### Role and policy changes

A session keeps its semantic version. It does not keep an old grant. When a role shrinks, affected sessions end, tokens are revoked, and the old pack is unloaded. Publish is: build an immutable candidate, close admission, apply grants and assets in the target transaction, wait for the replica barrier, then move the pointer. A failed step stays closed.

### Session release

Ending a session runs the whole S1–S11 release, not only a process exit. The design target is revocation within 5 seconds of entering release, and sandbox destruction within 60 seconds. Rust `Drop` does not destroy an external sandbox or revoke a remote token. A reaper must be able to finish a release after a crash.

### Private holdout

Public work uses synthetic fixtures. Customer samples, internal domains, production queries, gold holdout items, and secrets do not go into git, issues, pull requests, public CI, or an external review model. Holdout is scored aggregate-only. It is not used to tune individual items. A public release gate, when one exists, accepts a signed pass/fail and a version hash, not the questions or the traces.

## Review

The author is not the reviewer of their own security change. Roles are in [Spec §19](../specs/open-ontology-v0.1-spec.md) and [governance/review-guidance.md](../governance/review-guidance.md).

Low-risk documentation follows CODEOWNERS once that file is activated. It is not activated. See [governance/ownership-map.md](../governance/ownership-map.md). A file with path comments does not mean GitHub is requesting reviews.

## Dependencies

Read [supply-chain/policy.md](../supply-chain/policy.md) before adding a crate, package, image, or native library. Use the blank shape in [supply-chain/dependency-audit-record.template.json](../supply-chain/dependency-audit-record.template.json). Do not mark the template as audited. Do not edit `supply-chain/dependency-baseline.json` unless you are the foundation owner of that file and the record is real.

Default-prohibited licenses (AGPL, BSL, ELv2, SSPL) do not enter the default artifact. A version younger than seven days does not enter either, except through the security-fix exception in the policy.

## Pull requests and issues

Use the pull request template and an issue form under `.github/`. Vulnerability reports do not go through issues. [SECURITY.md](SECURITY.md) says where they go, and that the channel is not configured yet.

Evidence for a task lives at `evidence/<run-id>/<phase>/<task-id>/`. Public evidence is synthetic. A partial acceptance slice does not mark the whole acceptance case passed.

## Conduct

[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). The conduct contact is not configured. That is the same kind of release blocker as the security contact.
