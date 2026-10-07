# Cleanup

Checked after `linux/oracle-2` exited 0.

- Container `oo-a363-rules-smoke` was removed by its launch script (`linux/smoke/container-removed.txt`).
- Container `oo-a363-rules-proof` was removed by its launch script (`linux/proof/container-removed.txt`).
- Container `oo-a363-rules-oracle2` was removed by `linux/oracle-2/launch.sh` (`linux/oracle-2/container-removed.txt`).
- `docker ps -aq --filter label=oo.owner=a363` returned no ids after the oracle container was removed.
- Two short inventory probes used the same pinned Rust image with `--rm`, `--network none`, a read-only target mount, and no cargo volume. They exited before the oracle builder started. They did not write the volume.
- No PostgreSQL container was created or removed by this review.
- Network `oo-a363-eef831aa-net` was left in place.
- Volumes `oo-a363-eef831aa-cargo` and `oo-a363-eef831aa-target` were left in place. They hold the locked build cache and the rlibs the oracle linked.
- No `docker system prune`, volume prune, image delete, or foreign container removal was run.
- Host cargo, rustc, and clippy were not started. `linux/oracle-2/host-cargo-procs.txt` was empty and `capacity.txt` recorded `host_cargo=0 rust_builders=0 global=0 cap=10`.
- The sealed pass2 files and `tools/selfbuild-rules` were mounted read-only. They were not modified.
- `linux/smoke` and `linux/proof` logs were not overwritten. `linux/proof/launch.sh` still refuses to run when `console.log` exists.
