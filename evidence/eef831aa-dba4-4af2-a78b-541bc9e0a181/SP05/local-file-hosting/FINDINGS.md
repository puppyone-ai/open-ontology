# FINDINGS — a181 local file hosting

## Passed

The pre-edit `oo-control` smoke on Rust 1.94.0 exited 0 before `file_hosting.rs` existed.

The final smoke exited 0. Six integration tests passed against a real local object store: multipart bytes including non-ASCII and a NUL-free display name outside ASCII, reopen and resume, idempotent part and complete, read of the manifest and the bytes, abort, project quota, a conflicting part digest, a planted idempotency pointer whose session does not match the requested configuration, two contexts, checksum mismatch, size mismatch, and exact owned cleanup. The final object key was absent after both complete failures. A `notes` object under the part prefix and a file outside the store root remained.

`oo-arch-check` printed `architecture check: pass` with the locked graph. The required edges `oo-control -> oo-core` and `oo-control -> oo-jobs` are still in `Cargo.toml`. object_store is outside the forbidden driver sets. The gate was not weakened.

`nix` 0.31.3 with object_store's default features off compiled on linux/arm64. That is not a gate finding.

Lock checksums of the five added crates match the downloaded `.crate` files. Existing lock rows kept their checksums. Direct versions of sha2, getrandom, futures-util, and tokio were not bumped. The `fs` feature closure does not enable the cloud features.

## Properties the tests observed

`Denied` on a key the other context already holds confirms that the key is occupied. The error text is the fixed sentence `admission context does not match this upload` and does not include the owner's project or session id. A wrong digest on that session is `NotFound` for both contexts. Same bytes in a second project produce a different session and a different object key. The manifest debug text from the second project does not include the first project's id.

A resumed `begin` with a new grant string returns the same session. The completed manifest keeps the original grant. Quota ignores a non-hex name under `meta/quota/<project>/`. A completed object keeps its reservation. Abort of an open session releases it. Abort of a completed session is `Closed` and the object remains. A second abort is `NotFound`.

`HostLimits::try_new` rejects an object ceiling of `5_000_000_001`. A display name containing `/`, `\`, or `..` as a path, and a content type containing `..`, are refused before any object is written.

## Open

The local multipart handle is in memory. Durable resume is the part objects, not `MultipartUpload` itself. A crash can leave a `dest#N` file. This slice does not reap it.

Two `begin` calls can both pass the quota sum before either reservation is visible.

`PutMode::Update` is not used. The local store returns not-implemented for it.

S3, MinIO, the 5 MiB minimum part size, download, TTL, HTTP, authentication of the grant, MIME sniffing, formula-safe output, retention delete, and presigned URLs are not in this slice. SP05-W01 and SP05-W06 stay open. The ledger was not edited.

`cargo deny` was not run. Multiple itertools versions are a warn under `ci/deny.toml`, not a deny.

A known foreign key returns `Denied`, so occupancy of that exact key is visible to a caller who already has the key. Path secrecy is not the control.
