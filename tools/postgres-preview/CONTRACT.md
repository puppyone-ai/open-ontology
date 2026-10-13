# postgres-preview

Operator CLI for one native PostgreSQL 17.10 controlled preview. The binary orchestrates the source session, the frozen admission binary, and the later import stage. It does not link the live workspace crates.

## Config

Protocol `oo.postgres-preview.config/v1`. Unknown keys are rejected. A key named `password`, `bearer`, or `token` is rejected. Database and model secrets are file paths.

The model origin is `http://127.0.0.1:8317` and the path is `/v1/chat/completions`. The model id is `grok-4.7`. The bearer file for this deployment is `/tmp/oo-customer-demo-20261007/llm-token`. The CLI checks that the file exists and is at most 512 bytes. It does not print the file.

The source, DBA, and target usernames are three different logins. The DBA loads the logistics tables. The source login is the platform reader. The target login is the separate database.

## Commands

```text
postgres-preview <command> --config <file>
postgres-preview audit --config <file> --log <csvlog>
```

`dba-load` creates the logistics tables and the closed `jsonb` probe as the DBA, then grants the reader `SELECT` on the three logistics tables.

`run` opens one reader session, writes the session report, checks identity, types, row and byte caps, and the SQL oracle, then runs frozen `oo-source-admission validate`. While `execution/control-state.json` has no `development_extensions.DBX-W02-BUDGET.fixed_commit`, the command writes `dependency-pending.json` and exits 3. It does not start import, Builder, or preview, and it does not invent an admitted decision.

`oracle`, `type-gate`, `deadline`, and `cancel` are the same gates as separate commands. `deadline` expects SQLSTATE `57014` and no row. `cancel` cancels `pg_sleep` and checks that the backend is no longer active on that query.

`arrow-sample` writes three source shipments as Arrow IPC. The schema metadata `oo.postgres-preview.admitted_extract` is `false`. This is a type sample, not an admitted extract.

`write-option-probe` sends the same measured report to `validate` with `signal_table` true and requires `SIGNAL_WRITE`.

`target-partial` plants one unfinished target table and marks this run partial. A later `run` exits 9. `cleanup` drops that target table and marks the partial run cleaned. It does not drop the source.

`audit` runs `oo-source-admission audit-log` on the source csv log. A local classifier result is labeled `local_classifier_not_admission_binary` and is not a substitute for the binary.

## Exit codes

`0` command completed. `1` I/O or schema. `2` usage or an admission decision that is not the pending-producer case. `3` dependency pending or a missing frozen binary. `4` connect or TLS. `5` type gate. `6` identity. `7` audit. `8` oracle, deadline, or cancel. `9` foreign run or partial ingest. `10` row or byte cap.

## Import request

When a run is admitted and the recorded commit is present, the import body is `oo.pipeline.import-pg/v1` for frozen `oo-pipeline import-pg`. The password field is a file path. The frozen budget limits are 100000 rows, 33554432 bytes, and timeouts from 1 to 120000 milliseconds. This build does not call that binary while admission is pending.

## Frozen binaries

`bins.admission` is `oo-source-admission` from commit `fabc471609eedd313f7900009793ca153b61b187`. `bins.pipeline` is `oo-pipeline` from commit `7e4e42f14438f6d1bdeb84ffd936ed5b7192fd7f`. Builder and preview binaries are the frozen commits named by the task. The CLI does not compile the live workspace copies.

## Original PostgreSQL C1 page observations

`capture-receipt` and `capture-receipt-delta` emit `gN.capture.json` with protocol
`oo.postgres-preview.native-page-capture/v1`. These are producer observations,
not trusted admission receipts. Exported PostgreSQL snapshot tokens retain their
original value and `snapshot_token: true`; delta cursor descriptors use actual
positive decimal commit LSNs. S1 and `cross_table_consistent: false` remain explicit.
Existing DBA publication and logical slot use is R1. REPLICATION privileges must
remain visible in admission inputs. The product does not create either object.
The producer queries their actual names/owner/plugin/database and writes the raw
observation used by the descriptors' exact hash/path. Rollback instructions are
for the DBA; the product never executes them.

Target request identity and password file follow the operator's configured
restricted runtime principal. They no longer silently select `postgres` or the
superuser password. The target's actual TLS policy must still be configured in
the consuming operator request.

`linux/admission_inputs.py` prepares permission/config/witness/type inputs from
actual producer pages and source permission observations. It retains true
REPLICATION, requests R1, and issues no admission decisions. The authoritative
pinned source-admission executable must validate and seal the package before a
loader treats it as admitted.
