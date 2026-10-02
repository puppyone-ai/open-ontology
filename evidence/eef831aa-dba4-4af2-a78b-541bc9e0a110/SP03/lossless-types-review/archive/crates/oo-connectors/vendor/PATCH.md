# sqlx 0.9.0 private-CA and explicit-options patch

Upstream release: `sqlx-core` 0.9.0 and `sqlx-postgres` 0.9.0 (MIT OR Apache-2.0).
Identity: modified upstream artifacts. The directories started as the
crates.io 0.9.0 tarballs plus Hunk 1 and Hunk 2. Integration then replaced
the dangling `../LICENSE-MIT` and `../LICENSE-APACHE` pointers with the full
texts from SQLx VCS `003b698e99e024f3621b8043a2426fde5b741171` and added the
compile-time feature guard in `sqlx-core` `src/lib.rs`. An SBOM must name
this tree and the original tarball checksums. Version `0.9.0` does not mean
the bytes match the published crates. `[patch.crates-io]` in the workspace
root points only these two crates at this tree. No other driver is vendored.

`oo-connectors` enables sqlx features `runtime-tokio` and `postgres`, and
enables sqlx-core features `_rt-tokio` and `_tls-rustls-ring-no-roots`.
It does not enable `tls-rustls-ring-webpki`, `tls-rustls-ring-native-roots`,
`webpki-roots`, or `rustls-native-certs`.

## Why this is not a feature flag

sqlx 0.9.0 `tls-rustls-ring` is an alias of `tls-rustls-ring-webpki`.
`_tls-rustls-ring-webpki` always enables `webpki-roots`.
`_tls-rustls-ring-native-roots` always enables `rustls-native-certs`.
`_tls-rustls` alone does not select a crypto provider and does not compile.
`import_root_certs` returns `RootCertStore::empty()` only when both
`webpki-roots` and `rustls-native-certs` are off. The caller PEM is then
added. Cargo features are additive, so a public ring feature cannot be
combined with that empty store. GitHub issue 4033 asks for manual roots and
is not in 0.9.0.

## Hunk 1: `_tls-rustls-ring-no-roots`

`sqlx-core` Cargo.toml and Cargo.toml.orig:

```toml
_tls-rustls-ring-no-roots = ["_tls-rustls", "rustls/ring"]
```

`src/net/tls/tls_rustls.rs`: the ring provider cfg includes
`_tls-rustls-ring-no-roots`, and the aws-lc arm excludes it. The existing
empty-store function is the one that runs. A cfg(test) asserts
`import_root_certs().is_empty()` when this feature is on and the root
features are off.

## Hunk 2: `PgConnectOptions::from_explicit`

`sqlx-postgres` `src/options/mod.rs` adds `from_explicit()`. `new` and
`new_without_pgpass` copy `PGHOST`, `PGPORT`, `PGUSER`, `PGPASSWORD`,
`PGDATABASE`, `PGSSLROOTCERT`, `PGSSLCERT`, `PGSSLKEY`, `PGSSLMODE`,
`PGAPPNAME`, and `PGOPTIONS`. There is no public setter that clears
`options`, `ssl_client_cert`, or `ssl_client_key`. `from_explicit` builds
the same struct with those fields empty and does not read or modify the
process environment. The connector then sets host, port, username, password,
database, application name, `extra_float_digits`, `sslmode=verify-full`,
and the caller CA from its config.

## Not in this patch

No accept-invalid-certs path, no native-root fallback, no Mozilla bundle,
and no change to the sqlx facade's unconditional `migrate` feature.

## Integration license bodies

The published tarballs and the original correction tree ship `LICENSE-MIT`
and `LICENSE-APACHE` as the one-line pointers `../LICENSE-MIT` and
`../LICENSE-APACHE`. Those files now contain the full upstream texts.

| File | Bytes | SHA-256 |
|---|---:|---|
| `LICENSE-MIT` | 1109 | `5abbdd842310c8f1650fcde3a5a51a3e09412b9e5a6eea87519e0db9e32b339d` |
| `LICENSE-APACHE` | 10297 | `c8f5453612253e8ea8bad241617fe74c4a6d5d592ba64ff881668c211b50ac99` |

The same body is in both vendored crates. Original tarball checksums:
`sqlx-core` `05b44e85bf579a8eeb4ceaa77a3a523baf2bf0e9bac7e40f405d537b5d2d5ccb`,
`sqlx-postgres` `87a2bdd6e83f6b3ea525ca9fee568030508b58355a43d0b2c1674d5f79dcd65e`.
The accepted original unified patch is
`f4b1aff1f2575a6b161a5cec37da26a4e097475aa1dcde3a9cec603766ac5f37`.

## Integration feature guard

`sqlx-core/src/lib.rs` rejects, at compile time, `webpki-roots`,
`rustls-native-certs`, `native-tls`, `_tls-rustls-ring-webpki`,
`_tls-rustls-ring-native-roots`, `_tls-rustls-aws-lc-rs`, and
`_tls-native-tls`. Cargo feature unification makes the sqlx facade features
`tls-rustls`, `tls-rustls-ring`, `tls-rustls-ring-webpki`,
`tls-rustls-ring-native-roots`, `tls-rustls-aws-lc-rs`, and `tls-native-tls`
select those sqlx-core features, so the same guard rejects them. The
admitted feature remains `_tls-rustls-ring-no-roots`. A build that enables
the admitted feature together with `_tls-rustls-ring-webpki` fails this
guard. `rustls-platform-verifier` in another crate is not one of these
features.
