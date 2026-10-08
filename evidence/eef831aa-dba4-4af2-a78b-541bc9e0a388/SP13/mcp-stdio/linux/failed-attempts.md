# Failed attempts before the proof

These two smoke commands did not prove the Rust image. A login shell reset `PATH`, so `rustc` was not found. They are not a successful smoke.

## Attempt 1

Command shape: `docker run ... --entrypoint bash ... -lc 'rustc -vV; echo SMOKE_OK'`.

Output:

```
bash: line 1: rustc: command not found
SMOKE_OK
```

The shell printed `SMOKE_OK` after the missing compiler. That exit is not evidence that Rust 1.94 ran.

## Attempt 2

Same login shell, with `PATH` passed in the environment. `rustc` was still not found. Exit 127.

```
bash: line 1: rustc: command not found
```

Cause: `bash -lc` replaces `PATH` from the login profile. The image keeps `rustc` at `/usr/local/cargo/bin/rustc`, which is absent from that profile path.

## Attempt 3

Private PostgreSQL came up and libclang extracted. `cargo generate-lockfile` exited 101 before a child lock existed.

```
error: failed to parse manifest at `/src/tools/mcp-gateway/Cargo.toml`

Caused by:
  dev-dependencies are not allowed to be optional: `oo-gateway`
```

Cargo 1.94 rejects an optional dev-dependency. `oo-gateway` moved to an optional normal dependency gated by the `live` feature. The production build does not enable that feature. This attempt did not produce a child lock or a passing test.

## Attempt 4

The relaunch reached `docker run` for PostgreSQL and exited 125. Docker reported that the bind source `/tmp/oo-a388-eef831aa-dba4-4af2-a78b-541bc9e0a388/secrets` did not exist. The previous attempt's cleanup had removed that directory, and the new directory was not visible to the Docker Desktop mount. The secret directory now includes the launch pid so it is a new path. No test ran.

## Attempt 5

Phase ended at `clippy-protocol_exit=1`. Child lock generation, both fetches, and the production tree check had already succeeded. Root `Cargo.lock` stayed `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`.

```
error: 'cargo-clippy' is not installed for the toolchain '1.94.0-aarch64-unknown-linux-gnu'.
help: run `rustup component add clippy` to install it
```

The next run adds the Clippy component for that exact toolchain before the offline checks. This is not `cargo update` and does not edit the root manifest. No protocol test or live differential ran.

## Attempt 6

Phase: `clippy_component_exit=0`, then `clippy-protocol_exit=101`. The component command downloaded Clippy and rustfmt. `cargo clippy` then compiled `mcp-gateway` and stopped on a lint. Raw error:

```
error: returning the result of a `let` binding from a block
  --> src/http.rs:48:9
   |
47 |         let reply = self.exchange(path, Some(bytes)).await;
   |         --------------------------------------------------- unnecessary `let` binding
48 |         reply
   |         ^^^^^
   |
   = help: for further information visit https://rust-lang.github.io/rust-clippy/rust-1.94.0/index.html#let_and_return
   = note: `-D clippy::let-and-return` implied by `-D warnings`
```

No PostgreSQL from this card is still running. Cargo and target volumes `oo-a388-cargo`, `oo-a388-target-child`, and `oo-a388-target-root` remain. The return was inlined. The component install now matches the reviewed command `rustup component add clippy --toolchain 1.94.0`. Protocol tests and the live differential had not run.

## Attempt 7

Protocol-only, no PostgreSQL, same three volumes. Phase: `clippy_component_exit=0` (downloaded clippy only), `lock_present`, `fetch_child_exit=0`, `fetch_root_exit=0`, `tree_clean`, then `clippy-protocol_exit=101` in the same second. Raw error:

```
error[E0277]: `Session` doesn't implement `std::fmt::Debug`
   --> tests/protocol.rs:202:14
    |
202 |             .expect_err("unsupported version was accepted");
    |              ^^^^^^^^^^ the trait `std::fmt::Debug` is not implemented for `Session`
```

`Result::expect_err` requires the success type to be `Debug`. The rejection check now matches `Err` directly. The library had already passed this Clippy invocation. Live differential had not run.

## Attempt 8

Protocol-only again. No a388 container or PostgreSQL was left running. Volumes `oo-a388-cargo`, `oo-a388-target-child`, and `oo-a388-target-root` were reused. Phase: `clippy_component_exit=0`, `lock_present`, `fetch_child_exit=0`, `fetch_root_exit=0`, `tree_clean`, `clippy-protocol_exit=101`. Raw error:

```
error[E0716]: temporary value dropped while borrowed
   --> src/lib.rs:440:32
    |
440 |         let names: Vec<&str> = tool_list().iter().map(|tool| tool.name.as_ref()).collect();
    |                                ^^^^^^^^^^^ creates a temporary which is freed while still in use
441 |         assert_eq!(names, ["session_current", "pack_metadata", "query"]);
```

The tool list is now bound before the names are borrowed. Protocol tests and the live differential had not run.

## Attempt 9

Full differential on the existing volumes `oo-a388-cargo`, `oo-a388-target-child`, and `oo-a388-target-root`. No second builder and no leftover a388 PostgreSQL. `launch.exit=1`. Phase reached `clippy-protocol_exit=0`, `test-protocol_exit=0`, `build-gateway_exit=0`, `fixture_ready`, then `clippy-live_exit=101`. `test-live` did not start. Raw error:

```
error[E0507]: cannot move out of an `Arc`
   --> tests/live.rs:197:24
    |
197 |     let instructions = info.instructions.unwrap_or_default();
    |                        ^^^^^^^^^^^^^^^^^ ------------------- value moved due to this method call
    |                        |
    |                        help: consider calling `.as_ref()` or `.as_mut()` to borrow the type's contents
    |                        move occurs because value has type `std::option::Option<std::string::String>`, which does not implement the `Copy` trait
    |
note: `std::option::Option::<T>::unwrap_or_default` takes ownership of the receiver `self`, which moves value
   --> /rustc/4a4ef493e3a1488c6e321570238084b38948f6db/library/core/src/option.rs:1093:35
help: you can `clone` the value and consume it, but this might not be your desired behavior
    |
197 |     let instructions = <std::option::Option<std::string::String> as Clone>::clone(&info.instructions).unwrap_or_default();
    |                        ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++                 +
help: consider cloning the value if the performance cost is acceptable
    |
197 |     let instructions = info.instructions.clone().unwrap_or_default();
    |                                         ++++++++

error: could not compile `mcp-gateway` (test "live") due to 1 previous error
```

`peer_info()` is behind an `Arc`. The instructions line now borrows with `as_deref()`. The PostgreSQL data volume was removed by the launcher trap. The three cargo and target volumes remain.

## Attempt 10

The same three volumes. Launch pid 43116 exited after about 20 seconds. `launch.exit=1`. No a388 container remained. Phase: `clippy_component_exit=0`, `clippy-protocol_exit=0`, `test-protocol_exit=0`, `build-gateway_exit=0`, `fixture_ready`, `clippy-live_exit=101` at 08:45:48. `test-live` did not start. Raw error:

```
error: this assertion has a constant value
  --> tests/live.rs:63:5
   |
63 |     assert!(cfg!(target_os = "linux"), "the proc checks run on linux");
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   |
   = help: consider moving this into a const block: `const { assert!(..) }`
   = help: for further information visit https://rust-lang.github.io/rust-clippy/rust-1.94.0/index.html#assertions_on_constants
   = note: `-D clippy::assertions-on-constants` implied by `-D warnings`
```

The Linux check is now a const assertion. The three cargo and target volumes remain.
