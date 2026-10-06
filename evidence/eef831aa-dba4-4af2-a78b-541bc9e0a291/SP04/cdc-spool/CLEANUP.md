# Cleanup

Own resources removed after the inventory container exited 0. No `docker prune`. No foreign container, volume, or network was removed.

Removed:

- volume `oo-a291-eef831aa-cargo`
- volume `oo-a291-eef831aa-target`
- network `oo-a291-eef831aa-net` (no attached containers)
- directory `/tmp/oo-a291-eef831aa-dba4-4af2-a78b-541bc9e0a291`, including `precode/archive`

`docker volume ls --filter name=oo-a291-eef831aa` and `docker network ls --filter name=oo-a291-eef831aa-net` were empty after the removal. The private tmp directory was gone. No container with label `oo.kind=rust-builder` was running.

Named proof containers `oo-a291-impl-1` through `oo-a291-impl-7`, `oo-a291-manifest`, and the precode container were already removed by their launch scripts. Those `container-removed.txt` files remain in the evidence tree.

This file is a post-hash record. It is not a row in `MANIFEST.sha256`.
