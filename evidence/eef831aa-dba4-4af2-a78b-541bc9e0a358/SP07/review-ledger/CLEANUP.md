# CLEANUP

Owned resources only. No `docker prune`. No foreign container, volume, network, or image was removed.

Removed after the label `oo.owner=eef831aa-dba4-4af2-a78b-541bc9e0a358` was checked:

- container `oo-a358-review-pg` (`docker rm -f`)
- container `oo-a358-review-rust` was already absent
- network `oo-a358-review-net`
- volumes `oo-a358-review-pgvol`, `oo-a358-review-cargo`, `oo-a358-review-target`, `oo-a358-review-state`
- directory `/tmp/oo-a358-review-secrets`

A name diff of the host before and after showed one removed container, four removed volumes, and one removed network, all named above. No container was added. A later search for `oo-a358` in containers, volumes, and networks printed `none`. The secrets directory was absent after `rm -rf`.

`proof/cleanup-before.txt` is the owned listing taken before removal. It records the PostgreSQL mount sources `/tmp/oo-a358-review-secrets` and `deploy/trusted-demo/pg`. It does not contain passwords or keys.

Images were left in the local Docker store:

- `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`
