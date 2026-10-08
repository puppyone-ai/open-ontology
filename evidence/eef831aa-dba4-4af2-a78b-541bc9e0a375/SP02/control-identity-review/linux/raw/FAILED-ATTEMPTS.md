# Failed attempts

The final independent test and Clippy exits are 0. These earlier exits are kept.

## failed-attempt-1

First `run-proof.sh`. Smoke 0. Author tests 0 (11 library, 1 cli, 1 live). Author Clippy 0. `cargo-test-independent.exit` 101. The oracle crate failed to compile: `authorize` was defined twice, `SystemRandom::fill` needed `SecureRandom` in scope, the token handler's `text` string shadowed `text()`, `Some("other-client")` was not a `String`, a temporary `error_code` borrow, and `flush` was marked `mut`. `run-proof.exit` 1. Archive hash was unchanged.

## failed-attempt-2

Second builder, same cargo volume, same PostgreSQL. The crate compiled. `independent-proof.verdict` was already `WITHHOLD` and `binding_holds` was false. One assertion failed: `jwks_ambiguous_one_fetch` returned `token.rejected` because the previous mint was still the IdP `access_denied` mode. The test panicked after writing the proof. `cargo-test-independent.exit` 101. Clippy exited 1 because the new container did not have the clippy component (`cargo-clippy` is not installed for `1.94.0-aarch64-unknown-linux-gnu`).

## failed-attempt-3

Third builder. `cargo-test-independent.exit` 0. Verdict `WITHHOLD`, 68 cases, 0 failed, `binding_holds` false. `cargo-clippy-independent.exit` 101: unread field `cookie_header`, and two `clippy::collapsible_if` warnings, denied by `-D warnings`.

The following run installed clippy, retested, and finished with `cargo-test-independent.exit` 0, `cargo-clippy-independent.exit` 0, `rerun-independent.exit` 0, and the same withhold.
