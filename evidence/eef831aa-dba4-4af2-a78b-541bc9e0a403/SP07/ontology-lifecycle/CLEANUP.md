# Owned cleanup

Label `oo.owner=a403` and session `eef831aa-dba4-4af2-a78b-541bc9e0a403`.

Before removal: containers 0, volumes `oo-a403-eef831aa-cargo` and `oo-a403-eef831aa-target` (both labels `oo.owner=a403`), network `oo-a403-eef831aa-net`.

Removed those two volumes and that network. After removal, label `oo.owner=a403` counts are containers 0, volumes 0, networks 0.

No `docker system prune`. No foreign container, volume, or network was removed. Other cards' PostgreSQL containers and Rust builders were left running.
