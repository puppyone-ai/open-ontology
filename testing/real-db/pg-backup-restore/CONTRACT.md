# pg-backup-restore live harness

This directory runs the frozen protocol in `tools/pg-backup-restore/CONTRACT.md`. It does not accept SP14-W04, AT-OPS-06, or a parent goal.

The server wrapper `pg/pg-wrap.sh` is the reviewed a274 file, byte for byte, SHA-256 `d53f478805b2e721c81d5d8befafe70d4e6bac71af6136d2531149f70afe2bbb`. Launch names, the 1 GiB database limit, and the fixture are this session's. The client password is a mode-0600 file. It is not passed on a command line and it is not copied into evidence.

One Rust builder (6 GiB, 1 CPU) compiles and tests. It is removed before the database starts. One private PostgreSQL 17.10 (1 GiB, 1 CPU) then loads the fixture, takes the backup, marks the post-cut rows, refuses a missing manifest, a corrupt dump, and a corrupt manifest, and restores the sealed backup into fresh databases. The orchestrator is started through a private glibc 2.41 loader copied from the pinned Rust image, because the PostgreSQL image cannot start that binary. `pg_dump`, `pg_restore`, and `psql` are still the image binaries. A failed load is not resumed. The owned data volume is replaced.

Cleanup removes only containers, volumes, and the network whose `oo.owner` is `a356` and whose `oo.session` is `eef831aa-dba4-4af2-a78b-541bc9e0a356`. It does not prune.
