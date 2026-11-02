# Ownership map

Restoration note (2026-11-02): the presence/status column below is the dated
2026-10-01 inventory, not a current implementation inventory. Community files
are restored under `governance/`, not the root paths listed in that inventory.
CI, crates and deployments have since changed; this restoration does not audit
or accept them. Review roles and security separation requirements remain rules.

Status date: 2026-10-01.

This map is the review model for path domains. It is not an active GitHub CODEOWNERS assignment. No GitHub user, organization, or team is named, because this repository has no remote and no verified maintainer roster.

The author string on the existing design commit is repository metadata. It is not a code owner, a security contact, or a release contact.

## What is not in effect

| Control | State |
|---|---|
| `.github/CODEOWNERS` | Comments and path patterns only. No `@login` and no `@org/team`. |
| GitHub required reviews | Not in effect. A CODEOWNERS file does not create them. |
| Branch protection | Not configured. |
| Security owner | Not appointed. [docs/10](../docs/10-security.md) says the appointment belongs to M0. |
| Release / distribution contact | Not appointed. Publishing is blocked until one exists. |
| Private vulnerability and conduct channels | Not configured. See [SECURITY.md](SECURITY.md). |

## Activation, before anyone claims required review

All of the following have to be true:

1. The project has a real GitHub repository under an organization it controls.
2. Each role below is a real person or a real team, written as `@login` or `@org/team`.
3. Those tokens replace `OWNER_PENDING` in `.github/CODEOWNERS`, and the path lines are uncommented. More specific paths stay below broader ones, because GitHub uses the last match.
4. Branch protection on the default branch requires those reviews, including for administrators if that is the policy the project chooses.
5. [SECURITY.md](SECURITY.md) names a private vulnerability channel.
6. A pull request from the author of a security change still has a different reviewer. Branch protection is what enforces that. This document cannot.

Until then, treat review as a human request recorded in the pull request, not as a platform guarantee.

## Roles

These names come from [Spec §19](../specs/open-ontology-v0.1-spec.md) and the Plan. They are job labels. They are not GitHub teams.

| Role | Reviews |
|---|---|
| `rust-owner` | Crate boundaries, async resources, `unsafe`, maintainability |
| `security-owner` | Trust boundary, SQL, token, secret, network, native, plugins, release of sessions |
| `supply-chain-owner` | Dependency admission, SBOM, NOTICE, digest, prohibited licenses |
| `data-owner` | Types, positions, metrics, code lists, entities, calendars |
| `pipeline-owner` | Loader, CDC, snapshot, repair |
| `domain-steward` | Certified meanings, with the business owner of that metric or code |
| `ops-owner` | Deploy, backup, migration, recovery |
| `eval-owner` | Scorer, gold, holdout, with an annotator who did not write the gold |
| `release-owner` | Publication, signing, support matrix. Unappointed. |

One person may hold more than one role later. They still cannot approve their own security change.

## Path domains

Paths follow [docs/01 §7](../docs/01-architecture.md) and [Spec §4.2](../specs/open-ontology-v0.1-spec.md). The last column was corrected on 2026-10-01 after the lockfile, the architecture gate, the dependency baseline, and the governance examples note were already on disk. "Skeleton directory" means a crate directory exists and does not mean the crate implements its phase. This map does not audit those crates, the lock, or the baseline. A planned path stays listed so the domain is fixed before the code exists.

| Path | Boundary | Review roles | Present |
|---|---|---|---|
| `/LICENSE`, `/NOTICE`, `/README.md`, `/CONTRIBUTING.md`, `/CODE_OF_CONDUCT.md`, `/SECURITY.md`, `/SUPPORT.md` | Project license and community contract | `release-owner`, `security-owner` | yes |
| `/.github/CODEOWNERS`, `/.github/ISSUE_TEMPLATE/`, `/.github/pull_request_template.md` | Review templates | `release-owner` | yes |
| `/.github/workflows/` | CI workflows, when harness adds them | `rust-owner`, `security-owner` | no |
| `/governance/` | This map, ADR drafts, review guidance, support matrix | `release-owner`, `security-owner` | yes |
| `/supply-chain/policy.md`, `/supply-chain/dependency-audit-record.template.json` | Admission contract and blank record | `supply-chain-owner`, `security-owner` | yes |
| `/supply-chain/dependency-baseline.json` | Verified baseline. Foundation task (SP00-W01) writes it. Governance does not edit it and does not clear it. | `supply-chain-owner`, `security-owner`, `rust-owner` | yes, not audited here |
| `/docs/` | Design sources. Status stays Proposed until prototype evidence exists. | `rust-owner` for technical docs, `security-owner` for security docs | yes |
| `/specs/` | Spec, Plan, ledger. Not edited to fit a patch. | `rust-owner`, `security-owner` | yes |
| `/Cargo.toml`, `/rust-toolchain.toml`, `/.cargo/` | Workspace and toolchain. Observed from SP00-W01 while this map was written. Not reviewed by SP00-W06. | `rust-owner`, `supply-chain-owner` | yes |
| `/Cargo.lock` | Locked dependency graph. Presence is not a license clearance or a vulnerability scan. | `rust-owner`, `supply-chain-owner` | yes, not audited here |
| `/crates/oo-core/` | Types, schemas, errors. No service or driver deps. | `rust-owner` | skeleton directory |
| `/crates/oo-jobs/` | Control-database job queue | `rust-owner` | skeleton directory |
| `/crates/oo-control/` | Git, secrets, plan/apply, audit collector | `rust-owner`, `security-owner` | skeleton directory |
| `/crates/oo-connectors/` | Read-only sources | `pipeline-owner`, `data-owner`, `security-owner` | skeleton directory |
| `/crates/oo-pipeline/` | CDC, snapshot, loader | `pipeline-owner`, `data-owner` | skeleton directory |
| `/crates/oo-builder/` | Draft and compile. Generated vs curated. | `domain-steward`, `security-owner` | skeleton directory |
| `/crates/oo-gateway/` | SQL gate, grants, row-level security, audit | `rust-owner`, `security-owner` | skeleton directory |
| `/crates/oo-sdk/`, `/crates/oo-cli/` | HTTP clients. No direct database login. | `rust-owner` | skeleton directory |
| `/crates/oo-mcp/` | MCP transport and token exchange | `rust-owner`, `security-owner` | skeleton directory |
| `/crates/oo-proxy/` | Fixed egress, credential injection | `security-owner` | skeleton directory |
| `/crates/oo-sandbox/`, `/crates/oo-entry/` | Sandbox and session release | `security-owner`, `rust-owner` | skeleton directory |
| `/crates/oo-eval/`, `/bench/` | Public synthetic suites. Holdout is not this tree. | `eval-owner` | `oo-eval` skeleton only; `bench/` absent |
| `/crates/oo-analysis/` | Local analysis of an authorized export | `rust-owner`, `security-owner` | skeleton directory |
| `/crates/oo-arch-check/` | Workspace dependency-direction gate. The crate description says it is not a product service. Not in the docs/01 layout. Foundation owns the checker. This map does not audit its rules. | `rust-owner` | directory present |
| `/console/`, `/sdk-ts/` | TypeScript console and SDK. No database privilege. | `rust-owner` for the contract, console reviewer for UI | no |
| `/images/`, `/deploy/` | Images and Compose / Helm | `ops-owner`, `security-owner`, `supply-chain-owner` | no |
| `/ci/` | Gate manifest `ci/gates.yaml`, when harness adds it | `rust-owner`, `security-owner` | no |
| `/xtask/`, `/crates/xtask/` | Future xtask crate. [docs/01](../docs/01-architecture.md) does not pick which of these two directories. The task that creates the crate keeps one and drops the other pattern. | `rust-owner` | no |
| `/examples/README.md`, `/examples/governance/` | Synthetic illustration only. No program loads it. `examples/config/` and the config schema belong to the core owner and are not this slice. | `data-owner`, `security-owner` for the illustration; `rust-owner` for `examples/config/` and the schema | governance illustration only |
| `/evidence/` | Public synthetic evidence only. Historical runs stay immutable. | `release-owner` | public synthetic runs |
| `/execution/` | Coordinator dispatch notes. Not a product crate. | coordinator | yes |

Review combinations that Spec §19 already requires, whichever path the diff touches:

| Change | Reviewers |
|---|---|
| Gateway, grants, row-level security, proxy, identity, session release | component owner and `security-owner` |
| Loader, types, positions, cursors, repair | `pipeline-owner` and `data-owner` |
| Certified ontology meaning, code, anchor, entity rule | `domain-steward` and the business owner; `security-owner` when grants change |
| Native, `unsafe`, new dependency, hook | `rust-owner` and `security-owner` / `supply-chain-owner` |
| Migration, deploy, backup | `ops-owner` and the component owner |
| Scorer, gold, holdout | `eval-owner` and an independent annotator |

Low-risk documentation uses the CODEOWNERS path once it is active, without an extra approval layer. Dangerous operations still follow the product approval rules in the Spec. Customer sign-off is a customer role, not a row in this file.

## Holdout and private evidence

Private evidence, customer gold, and holdout items have no public path. They do not gain a CODEOWNERS entry by being committed. Do not commit them.
