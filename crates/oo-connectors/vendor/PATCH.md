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

## Hunk 3: opt-in incoming receive bounds

`sqlx-core` `src/error.rs` adds `ReceiveLimitKind`, `IncomingWireError`, and
`Error::IncomingWire`. The display text is a fixed label (`message`, `value`,
`row`, `columns`, or a short length class). It does not include server bytes,
a file path, or a length. `Error::as_incoming_wire` reads that payload.
`Error::Protocol` is unchanged and is not the carrier for these rejects.

`sqlx-postgres` `PgConnection` exposes `set_incoming_bounds`,
`incoming_bounds`, `incoming_allocation`, `incoming_synchronized`, and
`recv_backend`. `PgIncomingBounds::try_new` rejects a message cap below 4 or
a value cap above the row cap. The default is unset.

Unset keeps the previous path: buffer a whole backend frame, then decode it.
Two corrections apply in both modes. A length below 4 returns
`IncomingWireError::ShortLength` instead of panicking in `split_to`. A length
whose type-byte-inclusive end does not fit in `u32` returns `Overflow`.
`usize` overflow of that add is not hit on 64-bit targets.

When bounds are set, every backend format is checked against the message cap
before its body is reserved. DataRow lengths, NULL (`-1`), empty fields,
negative lengths below `-1`, column counts, row sums, truncation, and a body
that ends before the declared frame are checked from the bytes already
buffered. A reject does not ask the buffer to reserve the offending body.
Unknown type bytes become `BadType` and are not copied into the error.
Without bounds, an unknown type still returns the historical protocol error.

A receive-bound or length fault is stored on the stream. Later receives
return that fault and do not read. `wait_until_ready` checks the fault before
it flushes or receives, so `ping` cannot drain the socket. `set_incoming_bounds`
does not clear the fault. `close_hard` still shuts the socket down.
`recv_unchecked` keeps its signature. The retained buffer is the parser
state: it is consumed only after a full frame is accepted, with no await
between that decision and the split. Dropping the read future leaves the
bytes in place.

`incoming_allocation` reports `BytesMut` view capacities, reserve arguments,
view-capacity growth, read counts, overread, copied bytes, and accepted payload
lengths. The configured caps are not copied into those fields. `BytesMut::reserve`
is given the additional length beyond the buffer's current `len`. That argument
is `reserve_argument_bytes`. `reserved_additional` is the increase in the sum of
the two live view capacities and does not include an argument that left capacity
unchanged. Those no-op arguments are `noop_reserve_arguments`.

The two view capacities are not a physical resident total. They can share one
allocation, or not, and a caller-retained frame is not added in.
`accepted_backing_peak` is the same view sum sampled when a frame is accepted.
`rounding_peak` is spare above the reserve argument after a call that grew,
including the bytes 1.12.1 original-capacity floor when that floor is what the
buffer returned. It is not `malloc_usable_size`. Zero is recorded only when the
measured spare matched the argument.

`assembly_bytes` counts bytes copied when a retained prefix is relocated or when
`unsplit` cannot join the new spare in place. A contiguous join copies nothing.
Bytes already advanced past, sitting in front of both windows, are outside either
view and are not counted. Growth that keeps the same pointer adds nothing.
The diagnostic event log keeps at most 64 rows; `events_dropped` counts the
rest. A row is stored before the socket read is awaited, so dropping that read
leaves the growth and any copy that already happened on the counters and on the
row. `executor` portal execution is unchanged: `run` still sends no row limit
and an unnamed `Close`. This hunk does not add named portals, a stream reader,
or a product catalog cap.

No crates.io version is claimed for this hunk. `Cargo.toml` and `Cargo.lock`
are unchanged.

## Hunk 4: named portal on an open transaction

`PgConnection::open_named_portal` borrows an already open transaction
(`transaction_depth >= 1` and ready-for-query status `T`). Each `pull` reserves
one row from a caller-supplied positive allowance and writes `Execute` with
limit 1, then `Flush`. The driver reads that response until `PortalSuspended`,
`CommandComplete`, `EmptyQueryResponse`, or an error before it writes another
`Execute`. A returned row or a byte cap is not suspension and is not end of
results.

`Sync` and `Close` run when the portal completes or the caller closes it, and
when an extended-query error is followed by status `E`. That status leaves the
caller's transaction aborted so `ROLLBACK` can run. Any other failure, or a
cancelled `pull`, poisons the connection. A poisoned connection refuses ordinary
queries. Rollback of a poisoned connection does not write and does not panic.
This is not a claim that the backend process has exited.

Portal names use a separate counter from statement names. The portal path
refuses either counter when the next id would wrap. It does not insert the
statement into the statement cache. The connection is marked busy only after
`BindComplete`, so type resolution during prepare can still use the ordinary
executor. `run` is unchanged: unnamed portal, `Execute` limit 0, unnamed
`Close`, then `Sync`.

No crates.io version is claimed for this hunk. `Cargo.toml` and `Cargo.lock`
are unchanged.
