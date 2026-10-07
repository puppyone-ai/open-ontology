# Cleanup

Checked after the exit-0 proof.

- Container `oo-a354-rules-proof` is removed (`linux/proof/container-removed.txt`). `docker ps` for `oo.owner=a354` and `oo.kind=rust-builder` returned no ids.
- No PostgreSQL container was created.
- Network `oo-a354-eef831aa-net` was left in place.
- Volumes `oo-a354-eef831aa-cargo` and `oo-a354-eef831aa-target` were left in place. They hold the Clippy download and the locked build cache for this UUID.
- No `docker system prune`, volume prune, image delete, or foreign container removal was run.
- Host cargo, rustc, and clippy were not started. `linux/proof/host-cargo-procs.txt` was empty and `capacity.txt` recorded `host_cargo=0 rust_builders=0`.
- The sealed pass2 tree was mounted as three read-only files. It was not modified.
- Smoke logs under `linux/smoke` and `linux/smoke-clippy` were not overwritten.
