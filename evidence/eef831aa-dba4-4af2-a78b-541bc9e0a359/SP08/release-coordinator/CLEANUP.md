# CLEANUP

Owned resources only. No `docker prune`. No foreign container, volume, network, or image was removed.

Removed after the final status proof, in the previous turn of this session:

- container `oo-a359-pg` (`docker rm -f`), id `964da70de236d9ca1f971ee3d29119a9a8bb3b8e928a175026ed84b40438f39b`
- volumes `oo-a359-eef831aa-pgdata`, `oo-a359-eef831aa-cargo`, `oo-a359-eef831aa-target`
- network `oo-a359-eef831aa-net`, id `a1e3aa5070e5128ea1a42d6c96463d9066952f903d519247e160a2843de99db6`
- directory `/tmp/oo-a359-eef831aa-dba4-4af2-a78b-541bc9e0a359` (mode 0700 secrets: password files and private keys)

Builder containers named `oo-a359-build` were removed at the end of each run. None remained when cleanup of Postgres started. `linux/smoke/container-removed.txt` and `linux/smoke-attempt-2/container-removed.txt` record the smoke containers.

This continuation rechecked on 2026-10-08 and did not run `docker rm` again. `cleanup-after.txt` is that check: zero containers, zero volumes, and zero networks matching `oo-a359`, and the secrets directory absent.

Foreign containers seen during the recheck were left running. They belong to other sessions. Images were left in the local Docker store:

- `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`

PostgreSQL limits recorded before removal, `pg/inspect.txt`: `privileged=false`, `oom=false`, `memory=1073741824`, `cpus=1000000000`. `pg/ports.json` is `{}`. `pg/caps.json` drops `ALL` and adds `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`.
