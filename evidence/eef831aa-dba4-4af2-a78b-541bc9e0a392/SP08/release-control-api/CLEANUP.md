# CLEANUP

Owned resources only. No `docker prune`. No foreign container, volume, network, or image was removed.

Removed after `linux/proof/private-pg.json` and `linux/build-1/console.log` were on disk:

- container `oo-a392-pg` (`docker rm -f`), id `ee3ac54513273aa8ed462b904c2a0d2ea2e574f0b8a90c4be8f041750bce309d`
- volumes `oo-a392-eef831aa-pg`, `oo-a392-eef831aa-cargo`, `oo-a392-eef831aa-target`
- network `oo-a392-eef831aa-net`, id `4af561a64d51`
- directory `/tmp/oo-a392-eef831aa-dba4-4af2-a78b-541bc9e0a392` (password files and private keys, mode 0700, not committed)

Builder containers named `oo-a392-build` were removed at the end of each run, including the green run. None remained when Postgres was removed.

`linux/cleanup-after.txt` is the check after that removal: zero containers with label `oo.owner=a392`, zero volumes matching `oo-a392`, zero networks matching `oo-a392`, secrets directory absent.

Images were left in the local Docker store:

- `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`

PostgreSQL limits recorded before removal, `pg/inspect.txt`: `privileged=false`, `oom=false`, `memory=1073741824`, `cpus=1000000000`. `pg/ports.json` is `{}`. `pg/caps.json` drops `ALL` and adds `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`.
