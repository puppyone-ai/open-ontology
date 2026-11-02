# Source baseline revision 5: Gateway ACL documentation correction

Date: 2026-11-02 (Asia/Shanghai).

## Authority and provenance

The user authorized general gate remediation and prohibited forged history or
disabled checks. The detailed correction below was selected and reviewed by
the implementing assistant; the user did not specifically approve its contents.
This is not an independent review or product acceptance.

The input commit is `b495c9e0a1e51e9d707e60e795fd2d58f70430d9`.
All 19 source files were compared byte-for-byte with that commit before updating
the current manifest. No source document was changed to make a hash pass.

The exact prior manifest is preserved as
[sources-baseline-revision-4.json](sources-baseline-revision-4.json), SHA-256
`0d99205cae69754d0c3a7152cec6db3695a2db06107c90160a6aa6059d76dab7`.
Its original date, hashes, review status, and source commit are unchanged.
The [revision 4 decision](../execution/decisions/source-baseline-revision-4.md)
and historical failed checks remain historical; this correction does not
retroactively turn them into passes.

## Reviewed delta

Only `docs/07-gateway.md` differs from the revision 4 source manifest.
Compared with its source commit
`12cfdc48260636032429449d4e66253d54c1e3cd`, section 1 adds one paragraph
specifying `install-column-acl --previous-policy-file` and
`--previous-policy-sha256`.

The paragraph constrains native policy migration to the same endpoint, source,
schema, types, classification, role and login. It requires monotonic per-table
generations, exact old grants, transactional revoke/grant and rollback on
unknown or inconsistent authority. It keeps default installation strict, adds
no members or future-table privileges, and limits legacy metadata revocation
to the named NOLOGIN role while preserving login authority.

Impact review: these details refine the existing Agent DB role and column
permission boundary (REQ-DB-02, REQ-DB-03), Gateway role identity (REQ-GW-01),
and read-only execution (REQ-GW-04). They do not grant source write authority,
expand the query role, or waive permission tests. No requirement, acceptance
identifier, phase or work-item denominator is added, removed or signed off.
The other 18 source entries retain exactly their prior hashes and sizes.

## Current alignment

[sources-baseline.json](sources-baseline.json) records revision 5, the actual
source commit and freshly measured hash/byte/line values. The Spec points to
this correction; the Plan records the resulting Spec digest. Both retain the
existing product acceptance requirements.

Document checks validate traceability and consistency, not the runtime
correctness of the described ACL implementation. No database, customer asset,
historical evidence, crate or workflow is changed by this correction.
