# Notice — DL01-W1-10 Oracle target preparation

This directory does not contain Oracle Database binaries, Instant Client, or account credentials.

Oracle Database Free, as published by Oracle, is used under the Oracle Free Use Terms and Conditions (https://www.oracle.com/downloads/licenses/oracle-free-license.html, last updated 9 June 2021). The offering is the free on-premises program described in Oracle's licensing information (https://docs.oracle.com/en/database/oracle/oracle-database/26/dblic/Licensing-Information.html). Published capacity limits are 2 CPUs, 2 GB of RAM, and 12 GB of user data (https://www.oracle.com/database/free/).

The container image is Gerald Venzl's redistribution of that unmodified free program (`gvenzl/oracle-free`). The image build scripts are his project (https://github.com/gvenzl/oci-oracle-free). This repository does not redistribute the database image; a local pull records the digest. No Oracle account was created and no paid Oracle contract was accepted.

The preparation probe uses `sqlplus` that ships inside that database image. The product driver route, not compiled here, is Oracle's pure Rust thin driver `oracledb` 26.0.0-beta.4 (https://github.com/oracle/rust-oracledb), dual-licensed UPL-1.0 OR Apache-2.0. It does not require Instant Client. The crate is beta. Instant Client is not installed.

Diagnostic Pack, Tuning Pack, AWR, XStream, and GoldenGate are not enabled and are not queried. LogMiner is not started.
