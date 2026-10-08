# Failed attempts kept

| Run | Exit | Result |
|---|---|---|
| Ledger fetch before the workspace scaffold | 101 | `edition.workspace` had no parent `Cargo.toml`. Scaffold is `/tmp` only. Later fetch and build exited 0 and were not repeated. |
| Joint decide before the CSRF fix | 101 | `current_and_reject_are_durable` got 403 `csrf.rejected` after `identity_joint=current`. Caller JSON passed. Log: `failed-csrf-101/`. |
| Joint compile after the first CSRF edit | 101 | Three `match` expressions missed semicolons, and `cookie` was a `String` passed where `&str` was required. Log: `failed-compile-csrf/`. |
| Earlier full `cargo test` with the crate copied to `/tmp/crate` | 101 | `lock_subset` could not see the root lock. A later mounted run passed. That 101 is not a product failure. |
| First clippy component | 127 | `rustup` was not on the toolchain `PATH`. `/usr/local/cargo/bin/rustup` then exited 0. `rustc` stayed 1.94.0. |

The passing joint, contract, and clippy exits are the siblings of this directory, not these failed copies.
