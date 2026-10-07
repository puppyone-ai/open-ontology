# One handle observation

The 18-minute wait printed `OBSERVE_HANDLES` once. This is the single snapshot taken after that line. No second poll was started. No frontend files were edited.

Time of the docker listing: 2026-10-07T21:56:17Z.

Running containers:

| Name | Owner | Session | Kind | Status |
|---|---|---|---|---|
| oo-a362-smoke | a362 | eef831aa-dba4-4af2-a78b-541bc9e0a362 | rust-builder | Up about a minute |
| oo-a361-pg | a361 | eef831aa-dba4-4af2-a78b-541bc9e0a361 | postgres | Up 11 minutes, this review, removed by owned cleanup |

Exited containers from owner a064 were still present from five days earlier (`oo-a064-rust194-contract`, `oo-a064-rust194-contract2`, `oo-a064-pg1710-jobs`, `oo-a064-rust194-smoke`). They were left in place.

Running `oo.kind=rust-builder` count: 1 (`oo-a362-smoke`). Host `cargo`/`rustc`/`rustdoc`/`rustup` count: 0. This review's builder count at the snapshot: 0.

Session directories present for the nearby handles include a351, a352, a354, a356, a358, a359, a361, a362, a363, and a364. The a353 directory was not in the listing. Frontend work stays with a351, a353, and a352. This review did not open those trees.
