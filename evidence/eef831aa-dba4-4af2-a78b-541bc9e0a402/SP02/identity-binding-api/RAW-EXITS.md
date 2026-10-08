# Raw exits

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`.
PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`.
Toolchain inside the container: `rustc 1.94.0 (4a4ef493e 2026-03-02)`, `cargo 1.94.0 (85eff7c80 2026-01-15)`.

## Pre-edit smoke

`smoke/console.log`

```
fetch_exit=0
lib_exit=0
cli_exit=0
```

`smoke/lib-test.log`: `test result: ok. 11 passed`.
`smoke/cli-test.log`: `test result: ok. 1 passed`.

## First product compile

`compile-console.log`

```
control_test_exit=0
control_clippy_exit=1
binding_test_exit=0
binding_clippy_exit=1
binding_build_exit=0
```

Clippy had not been installed yet. See FAILED-ATTEMPTS 1.

## Final verification

`verify-console.log`

```
component_exit=0
control_lib_exit=0
control_clippy_exit=0
binding_clippy_exit=0
binding_build_exit=0
a370_exit=0
binding_loop_exit=0
```

`control-lib.log`: `test result: ok. 14 passed; 0 failed`.
`control-clippy.log`: `Finished dev profile`.
`binding-clippy.log`: `Finished dev profile`.
`a370-live.log`: `test result: ok. 1 passed; 0 failed; finished in 1.86s`.
`binding-loop.log`: `test result: ok. 1 passed; 0 failed; finished in 2.66s`.

## Final library and cli rerun

`final-console.log`

```
control_cli_exit=0
binding_lib_exit=0
```

`control-cli.log`: `test result: ok. 1 passed`.
`binding-lib.log`: `test result: ok. 3 passed; 0 failed; finished in 0.51s`.

Commands were `cargo test --locked --offline` and `cargo clippy --locked --offline --all-targets -- -D warnings`. No `cargo update`. No `cargo generate-lockfile`.
