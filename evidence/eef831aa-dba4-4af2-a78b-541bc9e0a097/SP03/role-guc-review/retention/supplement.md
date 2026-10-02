# a097 retention supplement

Session `eef831aa-dba4-4af2-a78b-541bc9e0a097`. Reviewer Grok 4.7 xhigh. This file is the retention record. `review.md`, `findings.json`, the original phase exits, and `cleanup.txt` are unchanged.

Limited decision: the original final oracle source is retained and byte-identical to `cleanup.txt` `oracle_rs_sha`. The original final oracle stdout and stderr were not recovered. A new, narrower oracle was run, and its source, full cargo stdout, full cargo stderr, integer exit, and hashes were saved before cleanup. That new run confirms the role-GUC, caller-CA, hostname, and admission observations, with a reader positive control and a source zero-write audit. It is a separate attempt. The prior scoped approve stands. SP03, PostgreSQL 13 and 14, types, bounded read, target import, CLI, supply, amd64, customer data, runsc, protected CI, and release stay unsigned.

## Original source

`cleanup.txt` records `oracle_rs_sha=fcdeaa943037efc827bf442e5dd1205aabe160f3bf56e7b3aae9cbec84337db8`.

The file was still on disk at `followup/a097_role_guc.rs`. An exact copy is `retention/original/a097_role_guc.rs`. Both measure 32240 bytes. Both hash to `fcdeaa943037efc827bf442e5dd1205aabe160f3bf56e7b3aae9cbec84337db8`. `retention/original/hash-compare.txt` records `byte_identical=yes`. The hash was computed on the existing bytes. The file was not rebuilt from notes or from a truncated tool transcript.

## Original final stdout

The original final oracle stdout and stderr are absent. Container `oo-a097-oracle2-eef831aa-dba4-4af2-a78b-541bc9e0a097` was removed in the original cleanup, and `/tmp/oo-eef831aa-dba4-4af2-a78b-541bc9e0a097` was removed with it. No complete stdout or stderr file from that process remains in this evidence directory.

`retention/partial-oracle2-tool-capture.txt` is a saved grep of a session terminal log, 476 bytes, SHA-256 `2a00a6bad637c67f66ec7b65fa0ed65b502aa9b8574ede13394201041b0c9ede`, 10 lines. It contains `running 2 tests`, `test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 14.15s`, `phase_exit=0 phase=oracle`, and `phase_exit=0 phase=lock`. It does not contain the test name `independent_role_guc`. It is a fragment of a filtered tool result. It is not the oracle process stdout.

`followup/phases/oracle-notes.txt` remains an observation note from that passing run. It is not the test source and not the test summary stream.

## New attempt

`retention/rerun/a097_retention_rerun.rs` is a new program, 13013 bytes, SHA-256 `d1a318b99d1a34f2c059f730d9aeb869872dd559907fdffbd89ceb0fbd79460e`. That hash is different from `fcdeaa94…`. The test function is `retention_role_guc_ca_host_admission`. It calls production `PgConnector` on the git archive of `34fae5ddd73ff250fde6f51aa681fb0f7fbd0362` (tree `c35b95664d53dcef1497bbd224ecbeec65586cea`).

Assertions in that source:

- One child, started with `Command::env`, sees `PGSSLMODE=disable`, `PGUSER=forged-user`, and `PGOPTIONS=-c role=oo_src_reader`, then still connects the reader under verify-full and is admitted with a `TLSv1.` version. The parent `PGSSLMODE` and `PGOPTIONS` stay unchanged. The child stdout lines that start with `a097_rerun ` are printed by the parent. Child stderr is not stored.
- Reader control: `registration_allowed`, session and current user `oo_src_reader`, mode `verify-full`, trust `caller-ca-only`, hostname verification on, negotiated version prefix `TLSv1.`, `server_observation=observed`, `version.admission=tested`, write `read_only`.
- `oo_src_guc_login`: session user is the login, current user is `oo_src_side`, blocks `role_mismatch/session_user` and `startup_option/startup_options`, negotiated version `unknown`, observation `unknown`, admission `tested`, major 17, minor 10, write `unknown`. Blocked `inspect_access` and `catalog` add no `app=oo-pg-source` statement or execute lines. Catalog error is `Config` / `session_user_mismatch`.
- A fresh GUC connection is blocked again with `unknown`. A fresh reader is admitted with `TLSv1.` and `read_only`.
- Wrong caller CA and host `oo-source-wrong` return `Tls` / `tls_handshake` with statement delta 0.

Printed result in the retained stdout:

```
a097_rerun child_reader_allowed=true tls=TLSv1.3 admission=tested
a097_rerun child_env=isolated
a097_rerun reader_allowed=true session=oo_src_reader current=oo_src_reader tls_mode=verify-full tls=TLSv1.3 obs=observed admission=tested write=read_only
a097_rerun guc_allowed=false session=oo_src_guc_login current=oo_src_side blocks=role_mismatch/session_user,startup_option/startup_options tls=unknown obs=unknown admission=tested write=unknown inspect_delta=0
a097_rerun guc_catalog_kind=Config fact=session_user_mismatch
a097_rerun guc_catalog_delta=0
a097_rerun guc_fresh_blocks=role_mismatch/session_user,startup_option/startup_options tls=unknown
a097_rerun reader_fresh_allowed=true tls=TLSv1.3 write=read_only
a097_rerun wrong_ca_kind=Tls fact=tls_handshake
a097_rerun wrong_ca_delta=0
a097_rerun wrong_host_kind=Tls fact=tls_handshake
a097_rerun wrong_host_delta=0
a097_rerun result=pass
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.38s
```

`trust=caller-ca-only`, hostname verification, major 17, and minor 10 are assertions in the source. They have no dedicated print line. The test process exited 0, so those asserts held. An independent `SHOW server_version` on the same fixture returned `17.10 (Debian 17.10-1.pgdg12+1)` and `server_version_num` `170010`.

This run did not repeat case-sensitive role names, `SET role = 'NONE'`, self-SET, user GUC, database GUC, database-user GUC, the allow-list pair, regression roles, serde, or the plaintext probe. Those observations stay in `review.md` and `followup/oracle-notes.txt`. This stdout does not re-prove them.

## Retained run output

Successful builder `oo-a097-ret-msrv-eef831aa-dba4-4af2-a78b-541bc9e0a097`: `status=exited`, `exit=0`, `oom=false`, started `2026-10-02T12:27:16Z`, finished `2026-10-02T12:27:40Z`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `RUSTUP_TOOLCHAIN=1.94.0`, `--pull never`, `--cap-drop ALL`, `no-new-privileges`, `--init`, private cargo and target volumes, `HOME=/tmp` inside the container. Fixture image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. One owned fixture, one owned builder. Launch capacity was `rust_running_count=0` and `host_rust_count=0`.

| Artifact | Bytes | SHA-256 | Secret-file hits | Canary hits |
| --- | ---: | --- | ---: | ---: |
| `rerun/phases/stdout.txt` | 1056 | `ace46cabc98a7e3874a45b812c98320db6e40f11098fade74a4aa3e7a0534b8d` | 0 | 0 |
| `rerun/phases/stderr.txt` | 6509 | `c74441931c8a98694d61bc909ae7f0d5f1a3e75b651e82c9fdb2cd6d096f1af3` | 0 | 0 |
| `rerun/phases/rerun.exit` | 2 | value `0` | 0 | 0 |
| `rerun/phases/container-stdout.txt` | 113 | `d68f565d3274a880f9905a647b10de233eb1dca58387f6f986d54e36e9df722f` | 0 | 0 |
| `rerun/phases/container-stderr.txt` | 0 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` | 0 | 0 |

`stdout.txt` and `stderr.txt` are the cargo test streams, redirected by `inside.sh` before the container exited. stderr is the compile log, ending at `Running tests/a097_retention_rerun.rs`, `Finished` in 21.78s. stdout is the libtest summary above. Wrapper `container-stdout.txt` is `phase_exit=0` for fetch, rerun, and lock.

Lock SHA-256 before the copy, after the test, and on the read-only archive are all `2b530fd334db74c992cd9238b4f71668ade554d4523c58782ca23c48ef0cd3e7`. `lock.exit` is 0. The archive mount was read-only. The test file was copied into `/tmp/work` inside the container.

## Harness failure kept

The first builder exited 1, `oom=false`, before `rerun.exit` was written. `cp -a /src /tmp/work` failed with `Operation not permitted` while preserving ownership under `--cap-drop ALL`. Fetch on that attempt exited 0. Preserved files: `harness-first-container.log` (148 bytes, SHA-256 `8481ea383d73f0d317397f9cd9551e066dc9d89fe6212347400568a1a44fe6a8`), `harness-first-container.exit` value `1`, `harness-first-fetch.exit` value `0`, `harness-first-fetch-stderr.txt` (9219 bytes, SHA-256 `aaf7271b322186e9576747ab9e5069c2f107146babdcfa6fe5785e8bced43620`). Secret-file hits on those files were 0. The copy was changed to `cp -a --no-preserve=ownership`. The failed container was removed so the same name could start again on the same fixture and the same cargo volume. The second start is the exit 0 above. No second fixture was created.

Earlier review failures stay in their original files: smoke package test and smoke final exit 101, first oracle compile exit 101, second oracle runtime exit 101, `check_plan.py` exit 1 for `docs/17-rust-implementation.md` hash and size drift. The baseline file was not edited.

## SQL audit of this run

`retention/sql-audit/postgresql.log` is this fixture only. The original `sql-audit/postgresql.log` still hashes to `3ce3e13600d662795ee081f9255be7994752caa44a9d78cd6607d5eb5ec20388` and is 1139003 bytes.

| Field | Value |
| --- | --- |
| bytes | 56944 |
| sha256 | `09db04f5b9113aae7c587995bc5337ae8655cda5ef9c89e4ac2c15bebc578587` |
| password_file_hits | 0 |
| canary_hits | 0 |
| password_word_lines | 0 |
| source_statement_lines | 66 |
| SELECT | 54 |
| WITH | 6 |
| START | 3 |
| ROLLBACK | 3 |
| other | none |
| preserved | raw |

Counts are lines with `app=oo-pg-source` and `LOG:  statement:` or `LOG:  execute`. Three `START` and three `ROLLBACK` match the three admitted inspects (child reader, parent reader, fresh reader). Blocked inspect, blocked catalog, wrong CA, and wrong host added none. Fixture `load.txt` is 1589 bytes, SHA-256 `711e484ae998c0a82ecfad67e4cd149e7e30400396ea748f2829acadec7d25f4`, secret-file hits 0.

## Cleanup

`retention/cleanup.txt` at `2026-10-02T12:32:11Z`. Removals exited 0 for the Rust container, the PostgreSQL container, network `oo-a097-ret-net-…`, volumes `oo-a097-ret-cargo-…`, `oo-a097-ret-target-…`, and `oo-a097-ret-pglog-…`, and `/tmp/oo-eef831aa-dba4-4af2-a78b-541bc9e0a097`. Later `docker inspect` of each of those objects exited 1. `tmp_present=no`. `docker ps`, `docker volume ls`, and `docker network ls` filtered by `a097` were empty. No prune. The original review containers were already gone and were not removed again.

## Boundaries left explicit

- Original oracle2 full stdout and stderr: missing.
- Partial tool capture and the new stdout: different artifacts from that missing stream.
- Child stderr for the env probe: not stored. The forwarded `a097_rerun` line and the child status are in `stdout.txt`.
- Case, `NONE`, self-SET, user and database GUCs, allow-list, regressions, serde, and plaintext: not re-run.
- Lib, doc, clippy, `live_pg`, and `check_plan.py`: not re-run.
- Vertical signatures in `findings.json` stay false. `merged_main_admission_slice_accepted` stays true, as already written.
- Coordinator assignment at this handoff: a098 owns root and the connector, a102 owns the new jobs migration, a103 owns the read-only CLI. This pass did not audit those trees. The older ownership sentences in `review.md` were left as written.
