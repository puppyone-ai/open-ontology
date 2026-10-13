# Notice — DL01-W1-34 Oracle driver preflight

This directory does not contain Oracle Database binaries, Instant Client, or account credentials.

Oracle Database Free, as published by Oracle, is used under the Oracle Free Use Terms and Conditions (https://www.oracle.com/downloads/licenses/oracle-free-license.html, last updated 9 June 2021). The container image is Gerald Venzl's redistribution of that unmodified free program (`gvenzl/oracle-free`, digest pinned in PIN.json). This repository does not redistribute the database image. No Oracle account was created and no paid Oracle contract was accepted.

The runtime driver is Oracle's pure Rust thin driver `oracledb` 26.0.0-beta.4 (https://github.com/oracle/rust-oracledb), dual-licensed UPL-1.0 OR Apache-2.0. The crate is beta. It does not require Instant Client. Arrow IPC uses the Apache-2.0 `arrow` crates locked in Cargo.lock.

SQL*Plus from the database image is used only to build the synthetic fixture and to read the audit trail. Diagnostic Pack, Tuning Pack, AWR, XStream, GoldenGate, and LogMiner are not enabled and are not queried.
