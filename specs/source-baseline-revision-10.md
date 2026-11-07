# Source baseline revision 10: Oracle LogMiner objects by version (D11a)

Date: 2026-11-04 (Asia/Shanghai).

Plan D7 and decision D11a. Lane E's T7 on Oracle 11g and 21c showed LogMiner's
own objects in SYSTEM rather than SYS and audit rows without recursion depth.
Input commit `a34c383bb47e846e35be2cade0faf2d736235a35`; manifest generated
by `specs/scripts/rebaseline.py`; prior manifest preserved as
[sources-baseline-revision-9.json](sources-baseline-revision-9.json).

Reviewed delta: `docs/product/02-ingestion-playbook.md` §7 names the LogMiner
objects per version and requires the default C5 path to pass a strict
zero-write T7 on its own. No requirement, acceptance case, invariant or phase
changes. No independent review or product acceptance is claimed.
