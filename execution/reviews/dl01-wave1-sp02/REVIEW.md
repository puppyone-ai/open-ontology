# DL01-W1-17 SP02 reconciliation

Session `add1dfc8-a1a3-4617-b890-dd9416161161`. Base `577160c22de63adb9b38e0147f3bc2da893c4d9d`. Admission observed HEAD `5fc39173c2de09c7239c01e823faf978409d4646`. Delivery parent `d7bccd02e8740085a2724c866e3fd1949a28f2f8` is later bookkeeping from other DL01 cards. Product trees below were compared at the base. From the base to that parent, the SP02 product paths are unchanged. `specs/open-ontology-v0.1-plan.md` differs only by the 2026-10-09 dispatch appendix after the SP02 section.

No SP02 parent is a candidate for formal signing. Every parent still has at least one original exit without evidence. Author delivery, limited child review, integration, and release stay separate. This packet does not edit Plan, ledgers, or the support matrix.

## Decision

| Parent | Formal sign | Why it stays pending |
|---|---|---|
| SP02 phase | no | Recoverable control operations, non-drifting input, measured secret rotation plus admin authorization, one consumed F1 contract, and a control-database backup are not all evidenced. |
| SP02-W01 | no | Schema, event binding, and SQL lease primitives have limited reviews. The Rust runtime review is `request_changes`. AT-CTL-01/02, backup, and target fencing are unsigned. |
| SP02-W02 | no | Strict YAML and AT-CTL-03 are open. The reviewed curated-Git adapter is `576e476f10eb6c9e3a06bdde7cc4f9e8b17bc9f6`. The base tree is the later unreviewed `d04da89ff696b45a4938dd59523821b5268b055f`. |
| SP02-W03 | no | No reviewed validate/plan/apply journey covers stale-plan rejection, per-step kill/restart, or same-key payload conflict. |
| SP02-W04 | no | Secret write/rotate/resolve is `APPROVE_LIMITED` at `bb831355662c1dffd76b86bf60417158f4513134`. Customer KMS, issued mTLS CA, pool refresh, Valkey namespace, and certificate expiry stay outside that verdict. |
| SP02-W05 | no | E54 `APPROVE_LIMITED` binds `a6f92da8a0bf872f8a30efb116c2c7b14463505b`. Base `tools/control-identity` is `5bc2350822db765610e3fdc6e4646fd2ec27c191`, with an author-only binding extension and no independent review. Customer IdP stays pending. |
| SP02-W06 | no | Feedback queue is `APPROVE_LIMITED` at `3be6264f18811b2ad88eedf986c342b15bdd0a16`. Gateway `report_gap`, full AT-CTL-06, and full AT-GW-06 stay open. The E55 collector seam is a SP06 child, not this parent. |

## DL01 registration and credentials

DL01 needs a person to submit connection facts, keep the password as a `secret://` reference, and have a later read use that reference. Three fixed pieces exist, and they are not one reviewed journey.

- `POST /control/v1/sources` at `fa921a03b9ccdf82c25e33c3aeaf9bfbc5712dd1` (`tools/source-control-api`) accepts `secret_ref` matching `^secret://[A-Za-z0-9_./-]{1,128}$`. The author contract says that reference names an operator file. The response never contains `secret_ref`, a password, a filesystem path, or argv. Keys `password`, `passwd`, `secret`, `authorization`, `credential`, `private_key`, `ca_pem`, and `api_key` are HTTP 400 `malformed`. Author packet `a383`, 91 SHA-256 entries, 0 bad. The handoff says independent review is still waiting. The tree does not call secret-control resolve.
- Secret resolve at `bb831355662c1dffd76b86bf60417158f4513134` is an independent `APPROVE_LIMITED` card. `POST /component/v1/resolve` requires `secret_ref`, `project_id`, and `namespace`. Its reference pattern is `^secret://[a-z0-9][a-z0-9_-]{0,62}/[a-z][a-z0-9_]{0,62}/[a-z][a-z0-9_]{0,62}/[a-z][a-z0-9_]{0,62}$` (deployment, project, namespace, secret id). Authority is the client-certificate URI `urn:oo:workload:…` read from the peer certificate. The registration service does not call this route. The two `secret://` grammars are different. The author example `secret://pg/a383/reader` does not match the resolve pattern.
- The source-control proof built control-identity from archive `a6f92da8a0bf872f8a30efb116c2c7b14463505b`, not from the base tree.

Pending exit for that DL01 slice: one fixed commit, then a separate review. Register stores a reference. The response still omits `secret_ref` and any secret value. The source worker calls the existing `POST /component/v1/resolve` with a client certificate whose URI equals the stored grant, and the body uses the four-part resolve reference. Mapping the looser registration pattern onto that four-part reference is part of the missing contract. Customer IdP, KMS, and mTLS CA issuance stay external.

Clause table: `CLAUSE-MAP.md`. Evidence packet: `evidence/add1dfc8-a1a3-4617-b890-dd9416161161/DL01/sp02-fixed/`.
