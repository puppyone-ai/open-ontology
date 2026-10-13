# Notice — DL01-W1-04 Oracle P2 child

This directory does not contain Oracle Database binaries, Instant Client, or account credentials.

The runtime driver is Oracle's pure Rust thin driver `oracledb` 26.0.0-beta.4 (https://github.com/oracle/rust-oracledb), dual-licensed UPL-1.0 OR Apache-2.0. The crate checksum in Cargo.lock is `d729181747bd69ba53cde2956ea9ddc06cf09d0badaf4a9e89944b7c0b7b4397`. It does not require Instant Client. Arrow IPC uses the Apache-2.0 `arrow` crates at 59.3.0 locked in Cargo.lock. `serde_json` 1.0.151 is MIT OR Apache-2.0.

The parent supervisor crate does not depend on this workspace. `oo-oracle-p2-decode` links Arrow only and does not link `oracledb`.

Oracle Database Free, when used by the proof harness, is the unmodified `gvenzl/oracle-free` image under the Oracle Free Use Terms and Conditions (https://www.oracle.com/downloads/licenses/oracle-free-license.html, last updated 9 June 2021). This repository does not redistribute that image.
