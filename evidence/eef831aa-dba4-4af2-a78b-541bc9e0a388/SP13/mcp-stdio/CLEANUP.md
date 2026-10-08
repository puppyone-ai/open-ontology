# Owned cleanup

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a388`. Cleanup ran in the launcher trap for pid 68660 after `linux/launch.exit` was 0. No foreign container, volume, network, or evidence directory was removed.

Removed by that trap:

- containers `oo-a388-rust` and `oo-a388-pg`
- volumes `oo-a388-pgdata`, `oo-a388-cargo`, `oo-a388-target-root`, `oo-a388-target-child`
- network `oo-a388-net`
- secret directory `/tmp/oo-a388-eef831aa-dba4-4af2-a78b-541bc9e0a388-<pid>` (passwords, CA key, server key)

Observed after the process exited:

- `docker ps -a` with label `oo.owner=a388` listed nothing
- `docker volume ls` listed no `oo-a388-*` volume
- `docker network ls` listed no `oo-a388-net`
- `/tmp/oo-a388-eef831aa-dba4-4af2-a78b-541bc9e0a388*` was absent
- host `cargo` / `rustc` were not left running by this card

The trap redacts password file contents and `ooct_` / `oost_` tokens from owned `*.txt`, `*.log`, `*.stdout`, and `*.stderr` before it deletes the secret directory. A later scan of those owned files found no remaining token or private-key marker.

Failed attempts 9 and 10 had already removed their PostgreSQL data volume and containers before the green run. Their cargo and target volumes were the ones reused, then removed on this exit 0.
