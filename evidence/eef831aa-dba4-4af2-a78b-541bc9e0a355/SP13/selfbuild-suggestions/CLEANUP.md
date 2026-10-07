# Owned cleanup

Done after `live/test.exit` 0 was copied onto the host and `live/record.txt` was written.

Removed:

- Container `oo-a355-sbs-live` (`f1ec86f0bf5569e6ac8ef17c99d55f68dc27d642c8ede0c274d4c72c11aeeaa6`), already exited 0.
- Container `oo-a355-pg` (`05e25c9e78cc8d0e80481b9aa5e3f7efaa90452e4fc8cf8a6096b112d58a38a0`), stopped, then removed.
- `/tmp/oo-a355-sbs-secrets` (passwords, CA key, server key, request copy). None of those files were in git.
- `/tmp/oo-a355-sbs-archive` (fixed-base source archive).
- `/tmp/oo-a355-watch-querypg.sh` and `/tmp/oo-a355-watch-live.sh`.

Left in place:

- Volumes `oo-a355-sbs-cargo`, `oo-a355-sbs-target`, `oo-a355-sbs-gw-target` (holds `debug/query-pg`), `oo-a355-sbs-pgdata`.
- Network `oo-a355-sbs-net`.

No `docker system prune`, no volume prune, no foreign container removal, no image deletion. After cleanup, `docker ps -a --filter name=oo-a355` is empty, running rust builders are 0, and host `cargo` / `rustc` / `rustdoc` / `rustup` processes are 0.

The query-pg binary was not committed.
