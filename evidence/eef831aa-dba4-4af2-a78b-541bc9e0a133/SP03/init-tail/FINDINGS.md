# Findings

`Read` on the slice from `BytesMut::init_mut` writes `n` bytes at index 0 and leaves the length at the capacity. `ReadBuffer::advance` then keeps a zero tail. The next `prepare_window` sees a full buffer and `bytes` 1.12.1 doubles the allocation. The message cap does not stop that. Workspace `Cargo.lock` pins `bytes` 1.12.1 (`fc652a48c352aef3ea3aed32080501cf3ef6ed5da78602a020c991775b0aff04`). The vendored `sqlx-core` lock still says 1.11.1 and was not used.

`ReadBuffer::read` now records `pre_len` after `prepare_window` and immediately before the socket await. When `socket.read` returns `Ok(n)`, `committed_init_len` runs before the EOF path and before `advance`:

- If `pre_len >= len_now` or `len_now - pre_len <= n`, the length is already an append (or unchanged) and is kept.
- Otherwise the spare was committed by `init_mut`. The kept length is `max(pre_len, n)`. That equals `pre_len + n` when either input is 0. When both are nonzero, `Read` overwrote the front of the whole slice, so the default tail starts at `max(pre_len, n)`, which preserves a prefix the write did not reach.
- The sum `pre_len + n` is not computed. Truncation does not change capacity, so the view meter still reports the live windows. Reserve growth stays charged. EOF and cancel do not roll it back. A cancel that drops the future before `read` returns does not truncate.

512 frames at 64-byte chunks stay at a live view of 8192 after the fix. The in-loop fuse is 24 KiB, under the old fourth-read peak of 32643. Cancel-after-`init_mut` and a no-data cancel each show one reserve to 16384, then the following 512-frame phase does not grow. That residual is inside the 24 KiB bound.

`cargo test -p sqlx-core` cannot run from this workspace. The package is a path dependency, not a member, and it has a tokio dev-dependency. `--features` is also rejected for a non-member. The embedded tests were run by copying the crate to `/tmp` inside the builder, dropping unused optional dependency tables from that copy, discarding the 1.11.1 lock, and pinning direct crates to versions already in the warm cache, with `bytes = "=1.12.1"`. The repo manifests were not changed. The isolated copy's empty feature table produced 151 `unexpected cfg` warnings during that test build. The product clippy run used the real manifest and exited 0.

The reader role cannot `SET log_statement`. The admin, which is a superuser, records `ALTER ROLE oo_a133_reader SET log_statement = 'all'` after the password statement. The password statement runs while the admin session has `log_statement = none`. The reader log then contains two `execute` lines, both selects, and zero write verbs.

The 60000-row TLS read observed `view_capacity_peak` 8192 and RSS about 14 MiB. The test fuse is 1 MiB for the view and 512 MiB for RSS. The view figure is the sum of the two live `BytesMut` capacities. It is not RSS. The rows held by `fetch_all` are caller memory, separate from that view. `oo-connectors` `MAX_ROWS_PER_SEC` of 50000 is a different path from this `PgConnection::fetch_all`.

The buffered correction is sufficient for the contract that was run: 512 small `init_mut` frames, the cancel and EOF regressions, and one private TLS read of 60000 small rows. No second product path was required.
