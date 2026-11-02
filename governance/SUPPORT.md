# Support

Restoration note (2026-11-02): the October 1 statements below describe the
original governance snapshot. A configured Git remote now exists; these files
do not verify a live issue queue or appoint support/security contacts.

> 2026-10-01。没有可支持的发行版，也没有支持联系人。

Open Ontology has no release and no supported version. There is no service level, no on-call rotation, and no production support desk.

## What you can read

| Question | Where |
|---|---|
| What the product is | [README.md](../README.md), [docs/00-vision-and-scope.md](../docs/00-vision-and-scope.md) |
| What must be true | [specs/open-ontology-v0.1-spec.md](../specs/open-ontology-v0.1-spec.md) |
| How implementation is supposed to proceed | [specs/open-ontology-v0.1-plan.md](../specs/open-ontology-v0.1-plan.md) |
| Which task owns which files | [specs/open-ontology-development-ledger.md](../specs/open-ontology-development-ledger.md) |
| What is actually implemented | [governance/supported-matrix.md](../governance/supported-matrix.md) |
| How to contribute | [CONTRIBUTING.md](CONTRIBUTING.md) |

The commands that exist today are the document checks in the README, including `python3 governance/check_governance.py`. That checker writes nothing unless its evidence flag is set. `./oo up` is a planned command. It is not a support path. The skeleton `oo` source says the command is not implemented.

## Defects and work items

Use the issue forms under `.github/ISSUE_TEMPLATE/` once a public tracker exists. This clone has no configured git remote, so those forms are the templates to publish with the repository. They are not a live queue.

Do not put customer data, secrets, or holdout items in an issue.

## Vulnerabilities and conduct

[SECURITY.md](SECURITY.md) and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

Both private contacts are unconfigured. That is a release blocker. Do not route either kind of report through a public issue, and do not invent a mailbox from git history.

## Publication and distribution

No person is authorized by this repository to publish artifacts or status pages. An unknown distribution contact is a release blocker, not a default assignee. Signing, an SBOM, and a per-artifact NOTICE start with the first M1 artifact. None of those exist.

## Customers

Customer onboarding in the Spec is a private record held by the customer project. It is not a file in this tree. Public support cannot accept a customer database, a sample extract, or a holdout set.
