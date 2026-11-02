# Governance

## Public-tree restoration (2026-11-02)

The generic SP00-W06 checker, its three QA modules, and governance documents
were restored from the original local checkout. No customer assets or runtime
evidence were copied. Community policies live here as
[CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md),
[SUPPORT.md](SUPPORT.md), and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md);
this scoped restoration does not install root-level GitHub community files.

The checker uses these explicit paths. README assertions now check the current
relational-storage definition instead of requiring the obsolete claim that no
implementation or CI exists. Git remotes are observed, not required to be absent.
Local links may target existing files or directories; missing targets still fail.

Cargo.lock is in the tree. It is not a license clearance and it is not a vulnerability scan.
`crates/oo-arch-check` is a workspace dependency-direction gate, and its crate description says it is not a product service.
Implementation and CI presence are not evidence of full product acceptance.

The [historical checker fixture](fixtures/check_sp00_w06.py) is a byte-identical
copy of the generic a002 checker, SHA-256
`3a8c642d47025093ea58ec116529a2ead60b7a01f08ce745e54bb30a3494d883`.
It is imported only to reproduce its eight known defects; do not execute its
historical evidence-writing entry point. Its source was
`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a002/SP00/SP00-W06/check_sp00_w06.py`.
The hash check and defect reproduction remain mandatory; private evidence paths
are not gate dependencies. This copy is not a new historical test result.

Run `python3 governance/check_governance.py` or
`python3 governance/check_governance.py --self-test`. The complete documentation
gate is `cargo xtask docs --check`. No check is bypassed by this restoration.

Files for SP00-W06. They set contribution, review, ownership, and supply-chain rules. They do not implement the product.

| File | Purpose |
|---|---|
| [ownership-map.md](ownership-map.md) | Path domains and review roles. Not a GitHub team list. |
| [review-guidance.md](review-guidance.md) | Reviewer checklist from Spec §19 |
| [adr-template.md](adr-template.md) | Draft shape for the next ADR in the existing ADR-NNN series |
| [supported-matrix.md](supported-matrix.md) | Honest status of specified capabilities |
| [check_governance.py](check_governance.py) | Document QA. The default run writes nothing. `--write-evidence` writes only when `--evidence-run` names the current permitted run. It refuses historical evidence, including session a007. Not the product test runner, and not `cargo audit`. |
| [../supply-chain/policy.md](../supply-chain/policy.md) | Dependency admission contract |
| [../.github/CODEOWNERS](../.github/CODEOWNERS) | Commented path template. Reviews are not active. |

Product definition, requirements, and acceptance cases stay in `docs/` and `specs/`. Do not edit those to match a governance draft.

## What the document QA actually checks

`python3 governance/check_governance.py` reads governance documents and the shape of an audit record. The default command writes nothing. Exit 0 means no applicable document check failed. It does not mean AT-SEC-05 passed, and it does not appoint an owner.

An audited record stores `publication_date` and `adoption_date` as full RFC3339 UTC timestamps (`Z` or `+00:00`), fractions allowed. A calendar date is rejected. It is not turned into midnight. `age_days_at_adoption` is the integer number of completed 24-hour intervals: the floor of the elapsed seconds divided by 86400. Admission compares the exact elapsed duration with `7 * 86400` seconds. `supply-chain/dependency-baseline.json` may still show a fractional age such as `7.142`. That float is baseline metadata. Copying it into an audit record does not satisfy the record, and this check does not rewrite the baseline. A baseline timestamp can be represented when the stored age is the recomputed floor.

`license_evidence[].source` has to be a path or an absolute `https` URL whose basename is `LICENSE`, `LICENCE`, `COPYING`, or `NOTICE`, with a suffix such as `.txt` or `-MIT`. The checker does not open the path, fetch the URL, or claim the file exists. A sentence such as "the manifest license field" or "no notice was read" is not a locator. Obligation text cannot be a placeholder (`n/a`, `none`, `pending`, `not applicable`, and the other empty words in [supply-chain/policy.md](../supply-chain/policy.md)). That is still record shape. It is not a license, vulnerability, or signature scan.

The operative-sentence check ignores fenced code and HTML comments, including a comment that runs across lines or never closes. A prefix such as "not", "isn't", "wasn't", "aren't", "won't", "can't", or "cannot" keeps that occurrence from counting. A negation in an earlier sentence does not hide the next one. The prefix list is small on purpose. It is not a proof of what English prose means.

A missing `yamllint`, and `git diff --check` while the worktree has untracked files, stay skipped. Skipped is not passed. Whitespace is a separate read of the file bytes. Nobody in this repository is an appointed owner, so a seven-day security exception does not pass.
