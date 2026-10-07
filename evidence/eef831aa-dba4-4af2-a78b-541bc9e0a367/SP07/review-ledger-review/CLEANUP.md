# CLEANUP

Owned resources only. No `docker prune`. No foreign container, volume, network, or image was removed.

`cleanup/before.txt` is the owned listing taken at `2026-10-07T22:52:47Z`, before removal. The PostgreSQL container id matched `pg/container.id`. Labels were `oo.owner=a367` and `oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a367`. The transcript names the secrets directory and omits the file names inside it. It does not contain passwords or keys.

Removed after those labels were checked:

- container `oo-a367-pg` (`docker rm -f`)
- containers `oo-a367-rust` and `oo-a367-smoke` were already absent
- network `oo-a367-eef831aa-net`
- volumes `oo-a367-eef831aa-pgdata`, `oo-a367-eef831aa-cargo`, `oo-a367-eef831aa-target`, `oo-a367-eef831aa-state`
- directory `/tmp/oo-a367-eef831aa-dba4-4af2-a78b-541bc9e0a367`, including its secrets, archive, and build tree

`cleanup/after.txt` at `2026-10-07T22:52:48Z` records `containers_none`, `volumes_none`, `networks_none`, and `tmp_absent` for the `oo-a367` names. Foreign builder `oo-a365-proof` (`oo.owner=a365`) was still running and was left in place.

Images were left in the local Docker store:

- `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`

`pg/secrets-path.txt` still names the removed directory. That path is the record of where the secrets were.
