# DL01-W1-34 Oracle native driver preflight

This directory is a standalone Rust workspace. It compiles Oracle's thin `oracledb` 26.0.0-beta.4 crate with the `arrow` feature and runs it against a fresh Oracle AI Database Free container. It does not accept an Oracle source-product card, does not certify 19c, and does not certify the marketing name "26".

`oracle-driver-probe` connects as `OO34_READER`, checks least-privilege grants, reads `OO34_FIX` as of an SCN, and writes `batch.arrow.partial`. `oracle-arrow-decode` does not link `oracledb`. The success file is renamed to `batch.arrow` only after the decoder and the cancel, privilege, and audit gates agree.

SQL*Plus is the DBA fixture and audit observer. It is not the driver. Instant Client is not installed. AWR, Diagnostic Pack views, LogMiner, XStream, and CDC are not opened.

Run `scripts/run-all.sh` on the host that already has the pinned linux/arm64 images. The script admits one 6 GiB / 1 CPU Rust builder and one 6 GiB / 1 CPU database under `/tmp/oo-dl01-wave1-resources.lock`.
