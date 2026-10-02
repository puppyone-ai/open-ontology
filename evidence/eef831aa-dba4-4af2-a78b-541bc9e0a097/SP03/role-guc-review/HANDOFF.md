# HANDOFF a097 role-GUC retention

Status: retention supplement is complete. No commit and no push. `review.md` and `findings.json` were not edited.

Provenance and the full hash table: `retention/supplement.md`.

## Decision

Scoped approve remains the decision already written for the role-GUC correction and the main source admission interface. `merged_main_admission_slice_accepted` stays true. `signs_sp03`, PostgreSQL 13 and 14, bounded read, types, CDC, loader, publishing, gateway, CLI, runtime, customer, CI, runsc, release, supply, amd64, target import, and the whole vertical stay false.

The original oracle source is retained and matches the recorded hash. The original final oracle stdout and stderr were not recovered. The new run is a separate, narrower oracle with its own hash and its own saved output.

## Original source and missing stdout

`followup/a097_role_guc.rs` and `retention/original/a097_role_guc.rs` are 32240 bytes. SHA-256 `fcdeaa943037efc827bf442e5dd1205aabe160f3bf56e7b3aae9cbec84337db8` equals `cleanup.txt` `oracle_rs_sha`. `byte_identical=yes`.

`retention/partial-oracle2-tool-capture.txt` (476 bytes, SHA-256 `2a00a6bad637c67f66ec7b65fa0ed65b502aa9b8574ede13394201041b0c9ede`) is a 10-line grep fragment: `running 2 tests`, `2 passed` in 14.15s, oracle and lock phase exits 0. It has no per-test name. It is not the oracle2 stdout.

## New retained run

Pin archive `34fae5ddd73ff250fde6f51aa681fb0f7fbd0362`, tree `c35b95664d53dcef1497bbd224ecbeec65586cea`. Linux arm64 Rust image `sha256:0f24958fa693…`, toolchain 1.94.0. PostgreSQL `17.10 (Debian 17.10-1.pgdg12+1)`, `server_version_num` 170010.

New source `retention/rerun/a097_retention_rerun.rs`, 13013 bytes, SHA-256 `d1a318b99d1a34f2c059f730d9aeb869872dd559907fdffbd89ceb0fbd79460e`.

Builder inspect: exited 0, oom false, `2026-10-02T12:27:16Z` to `2026-10-02T12:27:40Z`. `rerun.exit` 0. `lock.exit` 0. Lock before and after `2b530fd334db74c992cd9238b4f71668ade554d4523c58782ca23c48ef0cd3e7`.

| Stream | Bytes | SHA-256 |
| --- | ---: | --- |
| `retention/rerun/phases/stdout.txt` | 1056 | `ace46cabc98a7e3874a45b812c98320db6e40f11098fade74a4aa3e7a0534b8d` |
| `retention/rerun/phases/stderr.txt` | 6509 | `c74441931c8a98694d61bc909ae7f0d5f1a3e75b651e82c9fdb2cd6d096f1af3` |

Stdout ends with `a097_rerun result=pass` and `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.38s`. Observed: reader admitted, verify-full, `TLSv1.3`, observation `observed`, admission `tested`, `read_only`; GUC login `oo_src_guc_login` / `oo_src_side` blocked with both reasons, TLS `unknown` / `unknown`, admission `tested`, inspect delta 0, catalog `Config` / `session_user_mismatch`, catalog delta 0; fresh GUC still blocked; fresh reader admitted; wrong CA and wrong host `Tls` / `tls_handshake`, deltas 0; child env isolated. Secret-file hits 0 and canary hits 0 on both streams.

First retention builder exit 1 is preserved (`cp -a` could not keep ownership under `cap-drop ALL`; fetch on that attempt exited 0). The retry used the same fixture.

Not re-run here: case name, `NONE`, self-SET, user and database GUCs, allow-list, regressions, serde, plaintext, lib, doc, clippy, `live_pg`, and `check_plan.py`. Those remain in the original review files, including the smoke and oracle 101 exits.

## SQL audit

This run only: `retention/sql-audit/postgresql.log`.

- bytes `56944`
- sha256 `09db04f5b9113aae7c587995bc5337ae8655cda5ef9c89e4ac2c15bebc578587`
- password_file_hits `0`
- canary_hits `0`
- password_word_lines `0`
- source statements `66` = SELECT `54` + WITH `6` + START `3` + ROLLBACK `3`
- other verbs `none`
- preserved `raw`

The original audit file is unchanged: SHA-256 `3ce3e13600d662795ee081f9255be7994752caa44a9d78cd6607d5eb5ec20388`, 1139003 bytes.

## Cleanup

`retention/cleanup.txt`, `2026-10-02T12:32:11Z`. `docker rm` of the retention Rust and PostgreSQL containers, network, cargo volume, target volume, and pg log volume all exited 0. `rm` of `/tmp/oo-eef831aa-dba4-4af2-a78b-541bc9e0a097` exited 0. Each later `docker inspect` exited 1. `tmp_present=no`. No remaining `a097` container, volume, or network. No prune.

## Ownership

Coordinator assignment for the next work: a098 owns root and the connector, a102 owns the new jobs migration, a103 owns the read-only CLI. This retention pass did not review those surfaces. Older ownership sentences in `review.md` were left as written.
