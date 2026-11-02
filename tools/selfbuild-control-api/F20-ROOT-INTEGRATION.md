# F20 root integration requests (reported before any out-of-scope edit)

Root follow-up, 2026-10-27: items 1 and 2 below describe the original handoff
gaps. The local compile-resume caller now reruns the shared authorization
projection, with expiry/revocation regressions and no new S5 calls. ReviewLedger
and API preview now preserve the core `restricted` token and still reject
grant fields in classification decisions. Contract tests passed (10 ledger,
19 API), and the existing nine-scenario owned route fixture passed again.
Restricted-column full route coverage and published-runtime expiry are still
separate gates; these changes do not approve or install any customer grant.
Evidence: `evidence/convergence-20261027/coordinator/f20-local-resume-01.json`,
`restricted-contract-results.json`, and `f20-root-owned-02-result.json`.

1. **Local full-build compile resume**:
   `tools/pg-builder/scripts/library_build.py::stage` currently returns a cached
   compile receipt without invoking `library_control.run`. Before local resume
   is claimed to revalidate expiry/revocation, the owner must make the compile
   stage re-run the shared authorization/projection even when its receipt exists.
   Other observation/model stage caching must remain unchanged.
   Add an expired/revoked-policy resume regression. F20 changes this common
   compiler and HTTP cached GET/replay, but does not edit this read-only caller.

2. **Restricted classification**:
   `tools/review-ledger/src/fields.rs::classify` and the API's existing
   classification preview contract allow only none/personal/sensitive.
   Local pinned projection also supports restricted without downgrade.
   Restricted HTTP parity needs a coordinated ReviewLedger contract/test
   change before the API can accept that level. Do not map restricted to
   sensitive, bypass the ledger, or silently omit it.

3. **Installed caller inputs**:
   Both paths use the existing pinned `column_review_input`. Operators must
   install actual per-column grants with integer `read_grant_expires_at`, exact
   project/role/principal/current facts and unknown->sensitive reasons, update
   Library script/config pins and start a new reviewed build after input hash
   changes. No installed customer configs/services were changed by F20.
   No grant can be synthesized from a management role or visible-column list.

4. **Runtime/identity acceptance**:
   Formal route fixture evidence uses real owned PG and the pinned real ledger
   executable, but a TLS identity-contract fixture rather than production
   sessions. Real IdP/HTTP/customer acceptance, source-onboarding, publication
   and time-based revocation of already published Gateway/PG access remain
   external gates. Do not infer them from Pack equality.
