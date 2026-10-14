# DL01 SQL Server target preparation

Plan parent: DBX-W08. This directory prepares one SQL Server Developer container and the synthetic fixture around it. It does not promote the support matrix, and it does not claim CDC or Change Tracking.

The selected image is `mcr.microsoft.com/mssql/server:2022-CU27-ubuntu-22.04` at manifest `sha256:4402d880dd4c34bfa7d8705e56a86cd6c88da80a1f6bbbe741f999e76264a090`. The config blob says `linux/amd64`, product version `16.0.4295.3`. Machine-readable fields are in `PIN.json`.

This session ran on Apple arm64 (`uname -m` = `arm64`, Docker server `linux/arm64`). Microsoft publishes this repository as a single amd64 manifest. A `docker pull --platform linux/arm64` did not fail with a missing-manifest error; it began downloading that amd64 image. The pull was aborted. No container was started. x86 emulation is not the supported route.

An operator runs the fixture on a Linux x86-64 host:

1. `scripts/host-gate.sh` must exit 0. On this Mac it exits 42.
2. `scripts/run-amd64.sh` creates private password files outside the repo, starts one container named `oo-dl01w1-09-mssql` with memory cap `4g`, and applies the DBA scripts. Explicitly setting `OO_DL01W1_09_ACCEPT_EULA=Y` after operator authorization permits the runner to set `ACCEPT_EULA=Y`, which accepts the SQL Server EULA for Developer edition. Developer edition is for development and test, not production. This session did not accept the EULA.
3. The reader scripts then run as `oo_dl01w1_09_reader`. Positive reads must match `expected/oracle.json`. Negative writes must fail. Cancel drops the reader client during `WAITFOR`. The DBA login reads Audit and Extended Events afterward.
4. `scripts/cleanup-owned.sh` removes only the named container and its volume.

Identities:

| Login | Role on the disposable container | Used for |
|---|---|---|
| `sa` | image bootstrap | create the DBA login once |
| `oo_dl01w1_09_dba` | `sysadmin` on this empty container only | synthetic DDL, load, audit, XE |
| `oo_dl01w1_09_reader` | `SELECT`, `VIEW DEFINITION`, `VIEW DATABASE STATE` | catalog, oracle, denied writes, cancel |

The reader is the platform identity. The DBA login is not a model for a customer account. Production checklist text is comments in `dba/00-production-checklist.sql` and is not executed.

Driver route: tiberius `0.13.0`, MIT OR Apache-2.0, not compiled here. See `API-PROPOSAL.md`.
