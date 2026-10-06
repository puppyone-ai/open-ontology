# Cleanup

Own resources removed after the inventory container exited 0. No `docker prune`. No foreign container, volume, or network was removed.

Removed after `MANIFEST.sha256` was written:

- volume `oo-a299-eef831aa-cargo` (label `oo.owner=a299`, `oo.session=eef831aa-dba4-4af2-a78b-541bc9e0a299`)
- volume `oo-a299-eef831aa-target` (same owner and session)
- network `oo-a299-eef831aa-net` (same owner and session, 0 containers attached)
- host directory `/tmp/oo-a299-eef831aa-dba4-4af2-a78b-541bc9e0a299`

Named containers `oo-a299-preoverlay`, `oo-a299-overlay-1`, `oo-a299-overlay-2`, `oo-a299-overlay-3`, and `oo-a299-manifest` were already removed by their launch scripts. Those `container-removed.txt` files remain.

This file is a post-hash record. It is not a row in `MANIFEST.sha256`. The same is true of `linux/manifest/console.log`, `linux/manifest/docker-run.exit`, `linux/manifest/hostconfig.txt`, and `linux/manifest/container-removed.txt`.
