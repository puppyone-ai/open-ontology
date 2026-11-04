## Card B-S3b — lane B: rebase the SSH tunnel onto the integration branch

TODO: S3-A-01 (your S3 run is complete; evidence `b-s3-ssh-pg-full-02`, `b-s3-ssh-mysql-full-02`). The integration branch
moved under you: lane C added MySQL-family engines (`MysqlEngine.family`, family constructors), lane E added SQL Server
(doctor arm, `tools/oo-load` arm, tiberius in Cargo.lock), lane M added `oo-load sync`/publish structure. A coordinator
rebase conflicted in Cargo.lock, tools/oo-load/Cargo.lock, crates/oo-connectors/src/mysql/engine.rs,
crates/oo-cli/src/admin/doctor.rs and tools/oo-load/src/main.rs, so it is yours.
1. `git rebase v0.2/product-loops`; resolve keeping both sides: MySQL family field plus your tunnel; doctor keeps the
   SQL Server arm and routes it through the tunnel like Oracle (or refuses SSH for SQL Server by name, with a test);
   oo-load keeps lane C/E/M arms. Lockfiles: keep the integration side's versions and add your russh closure with the
   versions you audited (`cargo update -p <crate> --precise <v>`); run cargo deny + cargo audit. Every commit builds.
2. Apply your own license metadata patch (tools/oo-load, deploy/lite/mysql-doctor: `license = "Apache-2.0"`).
3. Re-run both SSH acceptance commands on the rebased tree (PG 17.11, MySQL 8.4.7) and the family/SQL Server unit tests.
4. Put the `oo init` TUI step text for lane A in the report (unchanged from your last report is fine).
