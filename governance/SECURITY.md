# Security

Restoration note (2026-11-02): this restores the October 1 reporting policy and
security requirements, not a current release/support assessment or runtime
audit. No security owner or private contact is appointed by this restoration.

> 2026-10-01。私人漏洞渠道尚未配置。不要开公开 issue，也不要向仓库历史里的提交作者发报告。

No release of Open Ontology exists. No version is supported. The design's security goals are in [docs/10-security.md](../docs/10-security.md) and [OO-SPEC-001](../specs/open-ontology-v0.1-spec.md) (requirements `REQ-SEC-01`, `REQ-SEC-02`, `REQ-SEC-03`, and the supply-chain rules in §4.1 and §20). Those documents are the target. They are not a statement that the controls are built.

## Reporting a vulnerability

Do not open a public issue, pull request, or discussion for a suspected vulnerability. Do not commit a proof of concept. Do not include customer data, production secrets, internal host names, or holdout material in a report.

### Private channel to configure before launch

This is a release blocker. Do not publish a binary, image, chart, or announcement while this section is still true.

Before the first public release, the first distributed artifact, or a public issue tracker:

1. Appoint a security owner. [docs/10](../docs/10-security.md) says this owner is to be named at M0. The security owner is not appointed. The author string on the existing design commit is not that appointment.
2. Publish one private channel in this file, in the same change that creates it:
   - GitHub private vulnerability reporting, after a real repository exists and the feature is turned on; or
   - a mailbox created for this project and read by the security owner.
3. Publish the conduct contact in [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). It may be the same channel.
4. Appoint a release contact who is allowed to ship artifacts and security notices. An unknown publication or distribution contact blocks release. Do not fill the name with a guess.
5. Keep the reports themselves out of git.

Until step 2 has happened:

- There is no address to mail.
- Do not use an address inferred from git history, a profile, or this text.
- If you already have a private path to the people operating a deployment, that path is outside this repository. This file does not create it and does not name it.

There is no bug bounty and no CVE process.

When a channel exists, a useful report names the component, the commit or artifact digest, the impact, and a synthetic reproduction. It omits customer samples. The project has not promised a response time. No such time has been agreed.

## Data residency and customer samples

Customer samples, production queries, gold labels, holdout items, and internal domains do not belong in this repository, in issues, in pull requests, or in public CI.

The design, in [docs/10 §8](../docs/10-security.md), requires each deployment to choose where model traffic goes before query results leave the network:

1. a private model endpoint, or
2. a contracted model service with zero retention and an acceptable region, or
3. another endpoint, only after an administrator records the risk.

If that choice is missing, the deployment does not send samples or results to an external model. The egress proxy that would enforce a model allow-list is specified and not present in this tree. Public development stays on synthetic fixtures. See the example in [README.md](../README.md).

## Triage, escalation, and safety gates

Levels from [docs/10 §13](../docs/10-security.md) and `REQ-SEC-03`:

| Level | Situation |
|---|---|
| P0 | Credential disclosure, an unauthorized read, or damage to a source database |
| P1 | Missing audit, or a sign that sandbox isolation has failed |
| P2 | Other security defects |

Intended escalation, once people are appointed: reporter, then the private channel, then the security owner, then the release contact if a shipped digest is affected. None of those people are appointed. There is no on-call rotation. Do not invent one.

Safety gates from the Spec:

- A failure of authorization, source non-write, credential isolation, durable audit, or required sync correctness is not waivable.
- A supply-chain age exception does not waive a known exploitable authorization, credential, or isolation defect. See [supply-chain/policy.md](../supply-chain/policy.md).
- A quality or performance waiver needs a named owner, an expiry, a monitor, and a repair task. That owner role is not staffed.
- A user-interface force option, when one exists, may cover only the non-security gates the Spec lists. The backend checks again.

No such console exists in this repository.

## Rendered HTML and CSV

`REQ-SEC-02` and [docs/10 §6](../docs/10-security.md) are the future bar for output handling: no external images, frames, or scripts from an answer; no automatic link previews; CSV and Excel cells that could be formulas get a protective prefix, including cells whose danger is only leading whitespace or a control character. Internal numeric Parquet stays numeric. The safe CSV text is not a second copy of the business value.

This repository has no HTML renderer and no CSV or Excel writer. It makes no claim that any rendered answer or exported file is safe. `AT-SEC-03` is the acceptance case, and it is not run.

## Emergency isolation

`REQ-SEC-03` describes emergency stop: close new sessions and data admission, revoke session and external tokens, cancel queries, drop proxy mappings, isolate and destroy sandboxes. A metadata-only mode still authenticates, redacts, and audits, and it does not dump the catalog. The pipeline can be paused on its own. Pausing a read does not remove source write-ahead-log or binlog retention risk.

That control is not implemented. There is no button and no rehearsed runbook. `AT-SEC-06` and `AT-OPS-05` are the later acceptance cases. This file is not evidence that isolation works.

## Audit integrity

The design requires an admission audit record before a result is returned, a hash chain so tampering is detectable, and a collector that is not a general write account on the control database. If the audit cannot be persisted, data admission stops.

None of that is running. Once the system exists, a gap in that chain is a P1. A log line in a future CI page does not replace the retained audit record.

## What this file does not certify

- No scan of a lockfile, image, or artifact has been claimed here.
- No penetration test has been run.
- Coordinated disclosure timing is unset, because the contact is unset.
- Publishing an advisory, a package, or a container is blocked until the security owner and the release contact are real.
