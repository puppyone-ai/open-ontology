# SP02 clause map at 577160c

Read from Plan SP02-W01–W06 and the phase line, Spec §6 and §17.4 and §18.2.1, docs 03 §3, docs 10 §7, docs 12 §3, docs 17 §3.1 and §6, and database-support README plus 01 §§2–4, 02 §4, and 05 §4. Admission observed HEAD `5fc39173c2de09c7239c01e823faf978409d4646`. Delivery parent `d7bccd02e8740085a2724c866e3fd1949a28f2f8` is later bookkeeping from other DL01 cards. From the base to that parent, Plan adds only the 2026-10-09 dispatch appendix after the SP02 section. Spec and those docs match the base for the clauses below. SP02 product paths are unchanged across that range.

Levels: `author` = author packet on a fixed commit. `review` = a separate review record of that commit. `integration` = a downstream product component consumes the same identities. `release` = declared operational gates. `pending` = an original condition has no such evidence.

No row is a formal parent signature.

## DL01 registration and credentials

| Clause | Requirement | Identity at base | Level | Result |
|---|---|---|---|---|
| docs 03 §3.2 / docs 12 §3 | Source instance is YAML applied by validate/plan/apply. `credential` is `secret://…`. | `tools/source-control-api` `fa921a03b9ccdf82c25e33c3aeaf9bfbc5712dd1`. OpenAPI `POST /control/v1/sources`, `secret_ref` pattern `^secret://[A-Za-z0-9_./-]{1,128}$`. The author contract resolves that reference to an operator file. The response omits `secret_ref`. Password-like keys are HTTP 400 `malformed`. | author | pending review. Does not call secret resolve. |
| docs 02 §4 | plan/apply starts a source worker. The worker resolves `SecretRef` and does not hold Agent DB, Gateway, or model credentials. | No call from `tools/source-control-api` or `tools/job-worker` to `POST /component/v1/resolve`. Registration and resolve use different `secret://` grammars. | pending | The worker resolve join is absent. |
| docs 10 §7 / REQ-SEC-01 | Source passwords live in an envelope store. Rotation covers pools. Values stay out of Git, logs, and images. | Secret card `bb831355662c1dffd76b86bf60417158f4513134`, review `a400` `APPROVE_LIMITED`. Tree matches the base. Customer KMS, issued CA, pool refresh, and Valkey are outside the verdict. | review, card only | pending W04 and pending the DL01 join. |
| docs 01 §4.1 | Registration compares detected version with the matrix. | `GET /control/v1/connectors` exists in the author API. Matrix grades were not promoted. No reviewed AccessReport comparison. | author | pending. |
| REQ-CTL-03 for the registrar | The caller is a management identity, not a data role. | Source-control proof used control-identity archive `a6f92da8a0bf872f8a30efb116c2c7b14463505b`. Base identity tree is `5bc2350822db765610e3fdc6e4646fd2ec27c191`. | review of the older tree; author on the base tree | pending re-review before this join. |
| PRODUCT-LOOPS DL01 | Password stays out of Pack, Git, and public logs. | Secret review recorded witness absence in its own logs. Registration proof is a separate author packet. No combined log scan is claimed here. | split | pending one journey. |

Proposed consumer call, not implemented by this card. The source worker calls existing `POST /component/v1/resolve` on `bb831355662c1dffd76b86bf60417158f4513134`. The body is `secret_ref`, `project_id`, and `namespace`. The reference matches `^secret://[a-z0-9][a-z0-9_-]{0,62}/[a-z][a-z0-9_]{0,62}/[a-z][a-z0-9_]{0,62}/[a-z][a-z0-9_]{0,62}$`. Authority is the client-certificate URI `urn:oo:workload:…` from the peer certificate, and that URI equals the stored grant. The register response continues to omit `secret_ref` and the resolved value. Password-like request keys stay HTTP 400 `malformed`. The registration pattern `^secret://[A-Za-z0-9_./-]{1,128}$` is a different grammar, and the author path currently resolves it to an operator file. A future card has to map one reference onto the other before this call can succeed.

## SP02 phase

Plan phase line: recoverable control operations, input that cannot drift, measured secret rotation and management authorization, an F1 contract, and a control-database backup marker.

| Exit piece | Evidence | Level | Result |
|---|---|---|---|
| Recoverable control operations | Job runtime author `1c4797e2f846` and review `a396` `request_changes`. Lease SQL `c312cae2daf7` is a limited primitive. | review of the primitive; runtime review requests changes | pending |
| Input cannot drift | Curated-Git review `a381` `APPROVE_LIMITED` at `576e476f10eb`. Base tree `d04da89ff696` differs. Full config Git CAS is open. | review of an older tree | pending |
| Secret rotation and admin authorization | Secret card limited. Identity E54 limited on an older tree. Binding author `5bc2350822db` has no review directory. | split | pending |
| F1 (Plan §3.2) | plan/apply, lease/generation, Connector/AccessReport, PositionCodec, and Batch are not one consumed contract. | pending | pending |
| Control DB backup marker | `contracts/jobs/control-schema` lists backup tables and says a backup was not run. `crates/oo-jobs/migrations/README.md` says the same. | author statement of absence | pending |

Integration for the phase: none of these outputs is consumed by Loader, Builder, and Gateway under one identity set. Release: customer IdP, KMS, protected CI, runsc, and dual-arch gates are named open by the reviews that exist.

## SP02-W01 control database and jobs

Plan execution: DDL and roles, then short-transaction claim, 30s lease, 10s heartbeat, generation CAS, schedule dedupe, database clock, NOTIFY as wake only. Exit: AT-CTL-01/02. A repeated schedule has one side effect. An old generation does not complete. Task state is not target fencing.

| Piece | Commit at base | Review | Hashes checked here | Level | Result |
|---|---|---|---|---|---|
| W01-a `jobs-control/1` | `70a4e72c3ea6`. `0001` blob `b6181331d68360e6eec5395b4400ba0141276aebffe21abc15c8b4b8f0310b75`, matches the author report. `0000` blob `445d7248…eb0b52`. Path diff versus that commit is empty. | `a068` `approve` for the schema slice. The base directory contains `review.md` and `findings.json` only. | no SHA256SUMS in that directory | review, slice | AT-CTL-01/02 unsigned in the review itself |
| W01-c event project | `9224d31e1c1d`. `0002` diff versus that commit is empty. | `a079` `approve` for the binding. A068-F2 closed for a LOGIN insert. F1, F3, F4, F5 left open. | no SHA256SUMS | review, slice | event content and the driver unsigned |
| W01-b1 SQL lease | `c312cae2daf7`. `0003` and `contracts/jobs/lease-operations` match. `crates/oo-jobs` last touch is this commit. | `a107` approve for `jobs-lease-operations/1` only. | no SHA256SUMS | review, primitive | Rust driver, 100 whole workers, NOTIFY, backup unsigned |
| Rust crate | `crates/oo-jobs/src/lib.rs` says claim, lease, and scheduling are not implemented in that crate. | — | source text | pending | pending |
| W01-b runtime | `tools/job-worker` `1c4797e2f846`, 26 files, diff versus base empty. Author evidence `a390` `evidence-sha256.txt` 371/0 bad. `closed-loop.exit`, `clippy.exit`, `test.exit`, `cleanup.exit` are `0`. | `a396` verdict `request_changes`. `evidence-sha256.txt` 223/0 bad. Handoff says the same person wrote the review. Jitter, `pg_notify` inside the seven functions, schedule payload column, business side effect, and OO016 overlap stay open. The 100-session author archive was not re-run. | 371 and 223 | author plus a `request_changes` record | pending. Not AT-CTL-01/02. |
| Backup / restore | Contract text says it was not run. | reviews refuse it | — | pending | pending |
| Applicator file hash and readiness conjunction | Plan checkbox still open. A068 residuals carried forward by a079. | — | — | pending | pending |
| AT-CTL-01 | 100 workers, lease expiry, old worker, unique target owner, old generation rejected, task and target checkpoints. | 100 sessions are an author archive of `jw` rows. Target owner and target checkpoint are named out of scope. | — | pending | pending |
| AT-CTL-02 | Multi-instance schedule, lost NOTIFY, retry/cancel/dead letter, business rollback, one side effect. | Author archive records schedule count 1 and a NOTIFY-loss poll. Review says the side effect is `jw.effects` txid insertion and the core functions do not `pg_notify`. | — | pending | pending |
| REQ-CTL-05 | Target fencing. Plan sends it to SP04. | every W01 review leaves it unsigned | — | pending | pending, and outside this parent’s sign-off |

`jobs.tasks` columns named in REQ-CTL-04 are in the schema contract. That column list is not AT-CTL-01.

## SP02-W02 config and Git

Exit: AT-CORE-02 and AT-CTL-03. Duplicate keys, tags, include, and environment interpolation are rejected. A conflict does not overwrite. A commit that cannot be bound does not enter plan.

| Piece | Identity | Level | Result |
|---|---|---|---|
| Curated-Git operator adapter | Review `a381` `APPROVE_LIMITED` at `576e476f10eb`. `MANIFEST.sha256` 205/0 bad. Receipt says `authenticated: false`. | review of that commit | E57 operator slice only |
| Base tree | `d04da89ff696` is the last commit on `tools/curated-git-apply`. Diff versus `576e476f10eb` changes `CONTRACT.md` and three schema files. | author, later | The limited approval does not cover the base tree. |
| Strict YAML / AT-CORE-02 | No packet here shows the full legal/duplicate/unknown/tag/include/depth suite for `oo.yaml`. | pending | pending |
| AT-CTL-03 | Concurrent edit, symlink, option injection, optimistic conflict, curated protected from Builder. | The operator adapter is a narrower CAS commit. | pending | pending |
| UI cannot bypass Git | Identity binding author writes a management-binding commit. Data-role grants are out of its scope. No independent review. | author | pending |

## SP02-W03 validate / plan / apply

Exit: AT-CTL-04. Kill during apply resumes without repeating resources. Same idempotency key and a different payload return conflict. A destructive operation without plan, permission, and risk text is blocked.

| Piece | Identity | Level | Result |
|---|---|---|---|
| Async 202, plan id, generation conflict, checkpoints | Not present as a reviewed control-plane apply service. | pending | pending |
| Source register / import | `fa921a03b9cc` has `POST /control/v1/imports` 202, cancel, and restart. Author `a383` SHA256SUMS 91/0 bad. Handoff: awaiting independent review. SP03, SP05, and B02 unsigned by the author. | author | pending. This is not AT-CTL-04. |
| Fake driver then real PostgreSQL recovery | Not found as a W03 packet. | pending | pending |

## SP02-W04 Secret and mTLS

Exit: AT-SEC-01. Wrong nonce, AAD, or key fails closed. Gateway cannot read the proxy credential namespace. Old credentials stop working. Rotation failure does not publish a plaintext session token.

| Piece | Identity | Level | Result |
|---|---|---|---|
| Write, rotate, revoke, resolve | `bb831355662c`. Author `a384` 172/0 bad. Review `a400` 110/0 bad, `APPROVE_LIMITED`. Tree matches base. | review, card | Card holds. Parent unsigned in the review. |
| AEAD, unique nonce, AAD, restart | Review F3 and F4: failed rotate leaves the prior version, distinct nonces, restart reads version 2 and refuses retired and revoked refs. | review, card | Holds inside the card. |
| Workload identity | Review F1: certificate URI, spoofed header ignored, foreign certificate fails before HTTP status. | review, card | Holds inside the card. |
| `key_source=kms` | Exits 2 `kms.external` before the key file is created. | external | pending |
| mTLS CA issuance | `mtls_ca=not_issued`. Test CA was minted by the review. | pending | pending |
| Gateway / Valkey namespace | Review F5: not in the card. | pending | pending AT-SEC-01 namespace leg |
| Pool and old credential coverage | `foreign_pool_refresh=not_claimed`. | pending | pending |
| 90-day DB password and 30-day certificate alarms | No packet. | pending | pending |
| Integration with source registration | No resolve call in `tools/source-control-api`. | pending | pending DL01 join |

## SP02-W05 OIDC and management RBAC

Exit: AT-CTL-05. Replay, cross-project, and self-promotion are rejected. A customer IdP stays pending. A test IdP is not customer acceptance.

| Piece | Identity | Level | Result |
|---|---|---|---|
| E54 code/PKCE/JWKS/RS256, cookie, bootstrap, CSRF, project guard, logout/restart | Original `a375` verdict WITHHOLD. Closure `APPROVE_LIMITED` at `a6f92da8a0bf`. SHA256SUMS: review 129, correction 96, closure 7, all 0 bad. Author repair `a379` 56/0 bad. | review of `a6f92da8a0bf` | Child holds. Full SP02 and customer IdP unsigned in the closure. |
| Base `tools/control-identity` | `5bc2350822db` adds binding routes and `oo_identity_control_v2.sql`. Diff versus `a6f92da8a0bf` is non-empty. | author | E54 approval does not cover this tree. |
| Binding preview/approve | Author `a402` SHA256SUMS 27/0 bad. Claims admin, reviewer, auditor, and business_owner on a test IdP, plus cross-project and CSRF rejection. No `identity-binding` review directory and no review commit. | author | pending review |
| Management role is not a data role | Author text says so. | author | pending review |
| AT-CTL-05 full route matrix | Secret review did not submit steward, auditor, or business_owner. Binding author is not a review. | pending | pending |
| Customer IdP | Named `not_claimed` by E54 and by the secret review. | external | pending |

## SP02-W06 collector and feedback receiver

Exit: a component slice of AT-GW-06 and AT-CTL-06. Management writes and credential reads are rejected. Gateway WAL end-to-end stays with SP06.

| Piece | Identity | Level | Result |
|---|---|---|---|
| Feedback receiver | `3be6264f1881`. Author `a385` 137/0 bad. Review `a393` 122/0 bad, `APPROVE_LIMITED`. Tree matches base. | review, queue slice | Full W06, SP13-W04, suggestion job, and Gateway `report_gap` stay open in that review. |
| Audit ingest / WAL | `1078d69e8c3c` for `tools/audit-ingest` and `tools/gateway-audit-wal`. Review `a380` 108/0 bad, `APPROVE_LIMITED` of E55. Tree matches. | review, SP06 child | The review says SP02-W06 is outside the verdict. |
| AT-GW-06 whole row | Collector disconnect, full disk, corrupt WAL, replay, admission before any result byte. E55 covers the frozen seam, including collector-failure retention and pre-admission fsync failure. | review, seam | The parent row stays pending. |
| AT-CTL-06 | Forged feedback target, oversize, injection, Gateway writing the control database, collector business writes. | Feedback review limits the queue. A universal business write was not accepted. | pending the rest of the row |
| Gateway has no control-database write account | E55 review states the gateway uses a client certificate to the collector. | review, seam | Holds inside E55. It does not sign W06. |

## Neighboring parents

| Parent | Relation | Result |
|---|---|---|
| SP03-W05 | Registration control API needs AT-CTL-04 and AT-SRC-04. | `source-control-api` is author-only and does not close those tests. |
| SP04 / REQ-CTL-05 | Target fence is a different parent. | Unsigned here. |
| SP06-W06 | Durable audit seam. | E55 `APPROVE_LIMITED` at an unchanged tree. Full SP06 stays open in that review. |
| DBX worker credential boundary (docs 02 §4) | P2 worker receives only the resolved source secret. | No reviewed worker join. |
| Support matrix | Grades stay at the documented values. | This card does not promote any cell. |

## Acceptance levels for the whole SP02 phase

| Level | Result |
|---|---|
| Author | Multiple fixed packets exist. They do not cover every original clause. |
| Fixed child review | Schema, event binding, SQL lease, E54 at `a6f92da8a0bf`, secret card, feedback queue, E55 seam, and curated-Git at `576e476f10eb` have limited records. Runtime review requests changes. Binding and source registration have no review. Two later trees drifted after an older approval. |
| Integration | No single import uses the base identity tree, the secret resolve listener, and `POST /control/v1/sources` together. |
| Release | Customer IdP, KMS, issued CA, backup/restore, protected CI, runsc, and dual-arch remain open. |
