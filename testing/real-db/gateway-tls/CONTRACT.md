# Gateway TLS harness

The frozen API is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a350/SP06/gateway-tls/CONTRACT.md`. This directory does not restate it.

`pg/pg-wrap.sh` is a byte-identical copy of the reviewed a326 wrapper. `pg/up.sh` and `pg/init-roles.sh` keep that recipe and change only the a350 names: database `oo_a350`, alias `oo-a350-pg`, SAN `DNS:oo-a350-pg`. The capability set is the reviewed one (`cap-drop ALL`, then `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETUID`, `SETGID`). It is not `--privileged`.

`linux/proof/launch.sh` starts that private PostgreSQL, then one 6 GiB arm64 Rust 1.94 builder. The builder archive is `HEAD` plus the uncommitted gateway TLS files. No registry crate is added.
