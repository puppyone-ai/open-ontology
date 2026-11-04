## Card M-S6c — lane M: Quickstart, command reference, troubleshooting, DBA grants

TODO: S6-M-02 (exit: a newcomer completes L1 from the docs alone). Rebase on v0.2/product-loops first (keep your S5/S6
commits). Granted: `docs/quickstart.md` (new), `docs/reference/cli.md` (new, generated), `docs/troubleshooting.md`
(new), `docs/database-support/dba-grants.md` (new), README.md "Getting started" section. Do NOT edit docs/*.md or
docs/product/*.md that are in the source hash baseline — propose changes to those in the report.
1. Quickstart (three minutes to a working Agent): install from npm, `oo init` (what each of the 7 steps asks), `oo up`,
   `oo build`, paste the share prompt into an Agent, `oo login`/`whoami`; lite local and `--target ssh`; the model
   key/base URL as secret files; what `oo status` shows. Every command must be copied from a real run
   (lane A4's evidence `a4-l1-20261105` and `crates/oo-cli/src/admin/RETAINED-BUILD.md` show the flow).
2. `docs/reference/cli.md` generated from `oo --help` and every subcommand's `--help` by a script
   (`tools/docs/gen_cli_reference.py`, with a `--check` mode wired into specs/scripts or ci) so it cannot drift.
3. Troubleshooting: every named error code the CLI/Gateway/loader can print on the L1 path (grep the code for
   `*_REQUIRED`, `OO0..`, `review_required`, `UPSTREAM_TIMEOUT` …) with cause and fix.
4. DBA grants per engine and version (MySQL 5.7/8.0/8.4/9.x incl. binlog, PostgreSQL 13–18 incl. logical replication,
   Oracle 11/21/23 incl. LogMiner opt-in D11a, SQL Server CT/CDC, MariaDB/TiDB/OceanBase, OLAP, openGauss) taken from
   each connector's `doctor` output contract — generate it from code where the GRANT lists live, not by hand.
Then do a docs-only dry run: in a fresh HOME follow docs/quickstart.md literally for PostgreSQL 17.11 and record any step
that did not work as written (fix the doc, or report the product gap).
Rebase note: your bundle commit 4b25daa65 conflicts in `deploy/lite/lifecycle.py` with lane A4's D16 provisioning
(`oo up` now provisions the retained chain and reports `retained_ready`). Resolve keeping both; rerun your bundle
tests (`test_bundle_dind.py`, arm64) on the rebased tree before the docs work, since the bundle must now also carry
A4's `install-retained-target` provisioning.
