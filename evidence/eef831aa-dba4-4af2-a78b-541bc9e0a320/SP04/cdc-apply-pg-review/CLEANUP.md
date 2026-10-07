# Cleanup

Own resources removed after the inventory container exited 0. No `docker prune`. No foreign container, volume, or network was removed.

Removed after `MANIFEST.sha256` was written:

- container `oo-a320-pg` (id `0a1225f7e3619e87191d9991756d51195c3dbca7de3972a1ea09d0b217fc9c28`, label `oo.owner=a320`, `oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a320`)
- volume `oo-a320-eef831aa-cargo` (same owner and session)
- volume `oo-a320-eef831aa-target` (same owner and session)
- volume `oo-a320-eef831aa-pgdata` (same owner and session)
- network `oo-a320-eef831aa-net` (same owner and session)
- host directory `/tmp/oo-a320-eef831aa-dba4-4af2-a78b-541bc9e0a320` (CA key, server key, and password files)

Named containers `oo-a320-preoverlay`, `oo-a320-overlay`, and `oo-a320-manifest` were already removed by their launch scripts. Those `container-removed.txt` files remain.

`oo-a300-pg`, `oo-a317-pg`, and `oo-a321-pg` were still running after this cleanup.

The clarification after those removals starts no PostgreSQL and removes no foreign resource. `linux/manifest/refresh.sh` runs one pinned-image hasher and removes that container. `linux/manifest/MANIFEST-first.sha256` keeps the first inventory. The rewrite of `MANIFEST.sha256` happens after the child-verdict text. These files are copied after that rewrite and are not rows in it: `linux/manifest/refresh-console.log`, `linux/manifest/refresh-hostconfig.txt`, `linux/manifest/refresh.exit`, and `linux/manifest/refresh-removed.txt`. `MANIFEST.sha256` is the output path and is not a row of itself.

The first inventory's `linux/manifest/console.log`, `docker-run.exit`, `hostconfig.txt`, and `container-removed.txt` remain. They were outside the first inventory. The clarification rewrite includes them because they already exist.
