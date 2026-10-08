# Failed attempts

The successful logs replaced `binding-loop.log` and `a370-live.log`. The lines below are the assertions read from those files before the replacement. `live-console.log` still has the first loop exit.

## 1. Clippy component missing

`compile-console.log`: `control_clippy_exit=1`, `binding_clippy_exit=1`. Both logs said `cargo-clippy is not installed for the toolchain '1.94.0-aarch64-unknown-linux-gnu'`. Library tests in that same run were already green (`control_test_exit=0`, `binding_test_exit=0`, `binding_build_exit=0`). The next container ran `rustup component add clippy --toolchain 1.94.0` (`component_exit=0`) and then clippy.

## 2. Database role name

`docker exec -u postgres psql` without `-U` returned `FATAL: role "postgres" does not exist`. `POSTGRES_USER` is `oo_fixture_admin`, and local socket auth is trust. The retry `psql -U oo_fixture_admin` applied the bootstrap SQL (`bootstrap_exit=0`).

## 3. Approve `git.rejected`

`live-console.log`: `a370_exit=0`, `binding_loop_exit=101`. The loop panic was:

```
assertion `left == right` failed: approve {..."error":{"code":"git.rejected","message":"the config git commit was rejected"...}}
  left: 403
  right: 200
```

`binding_head.revision` stayed 1. `git_commit` length stayed 0. No `binding_applied` audit row. `git-shape.txt` shows `ls-tree` listing the `management` tree and `ls-tree -r` listing the blob. Fixed in `tools/control-identity/src/git.rs`. See FINDINGS F1.

## 4. a370 saw a leftover schema

The next verify, before a manual `DROP SCHEMA`, recorded `a370_exit=101`:

```
assertion `left == right` failed
  left: String("replayed")
  right: "committed"
```

at `tests/live/main.rs` install. The schema was the one installed by attempt 3. The a370 test drops `oo_identity_control` only after that assertion. The final run dropped the schema first. `a370_exit=0`.

## 5. Self-revoke expected frozen

That same verify recorded `binding_loop_exit=101`:

```
assertion `left == right` failed
  left: "binding.elevation"
  right: "binding.frozen"
```

at the revoke of `fixture-admin` by the `fixture-admin` session. Elevation is first in `plan`. The test now expects elevation for that call and uses a second admin, `person-deputy`, for `binding.frozen`. The final loop exited 0.
