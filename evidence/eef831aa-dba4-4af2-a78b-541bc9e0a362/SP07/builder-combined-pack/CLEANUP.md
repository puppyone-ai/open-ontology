# Cleanup

Removed only resources labeled `oo.owner=a362` for session `eef831aa-dba4-4af2-a78b-541bc9e0a362`.

Containers were already removed by each launch script after the run (`oo-a362-smoke`, `oo-a362-smoke2`, `oo-a362-proof`, `oo-a362-proof2`, `oo-a362-proof3`, `oo-a362-proof4`). None remained at cleanup.

Removed volumes:

- `oo-a362-eef831aa-cargo`
- `oo-a362-eef831aa-target`

Removed network:

- `oo-a362-eef831aa-net` (`ae35aab160b0`)

Removed `/tmp/oo-a362-eef831aa-dba4-4af2-a78b-541bc9e0a362`.

Left in place: container `oo-a366-build` (`2af071161443`), owner `a366`, session `eef831aa-dba4-4af2-a78b-541bc9e0a366`, status exited 0. No prune, no foreign volume, and no foreign network were touched.
