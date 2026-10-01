# Synthetic license fixtures

Each directory is its own Cargo workspace with no dependencies. None of them is a member of the product workspace. The license expressions are synthetic. They are not the `ring`, `rustls-webpki`, or `untrusted` sources.

cargo-deny 0.20.2 command, from the repository root, with a temporary `CARGO_HOME`:

```
cargo-deny --format json --manifest-path contracts/supply-chain/isc/fixtures/<name>/Cargo.toml --config ci/deny.toml --locked check
```

Exit 4 means the license check failed. It does not mean four errors. The measured candidate results are in [policy.md](../policy.md). `positive-isc` fails on the allow list without `ISC` and passes after `ISC` is added. The GPL-only, AGPL-only, SSPL-only, BUSL-1.1-only, BSL-1.0-only, unknown, and LLVM-exception-only fixtures fail on both allow lists. `dual-mit-or-gpl` and `exception-or-apache-mit` pass on both. `mit-or-gpl-missing-mit-text` also passes on both, and its tree has no MIT license text.
