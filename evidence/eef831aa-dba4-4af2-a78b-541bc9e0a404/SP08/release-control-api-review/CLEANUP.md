# CLEANUP

Owned resources only. No `docker prune`. Images were left in the local store. Foreign containers were not named by the cleanup command.

Removed after the proof:

- Container `oo-a404-pg` (label `oo.owner=a404`, session `eef831aa-dba4-4af2-a78b-541bc9e0a404`). Id before removal `9d36222b1560dd7cdd860d08e45e8bdb1611923aff148643eedbfd768b09076d`.
- Builder `oo-a404-build` was removed by `oo-a404-build.sh` when the run exited 0.
- Volumes `oo-a404-eef831aa-pg`, `oo-a404-eef831aa-cargo`, `oo-a404-eef831aa-target`.
- Network `oo-a404-eef831aa-net`.
- Directory `/tmp/oo-a404-eef831aa-dba4-4af2-a78b-541bc9e0a404` (password files, TLS keys, and the git archives).

Afterward, `docker ps -a --filter name=oo-a404` was empty, the three volumes were gone, the network was gone, and the tmp directory was gone. Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5` and PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb` were still present.

Postgres containers still running after this cleanup, untouched by it: `oo-a402-pg`, `oo-a397-pg`, `oo-a398-pg`, `oo-a383-pg`, `oo-a373-pg`.

Password scan of the evidence tree, while the secret files still existed, found no password bytes. Those files were deleted with the tmp directory.
