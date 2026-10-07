# One handle observation

The 18-minute wait printed `OBSERVE_HANDLES` once, at 2026-10-07T22:35:39Z. This is the single snapshot taken after that line. No second poll was started. No frontend files were edited.

Time of the docker listing: 2026-10-07T22:36:09Z.

Running containers:

| Name | Owner | Session | Kind | Status |
|---|---|---|---|---|
| oo-a366-pg | a366 | eef831aa-dba4-4af2-a78b-541bc9e0a366 | postgres | Up 14 minutes, this review |

Exited containers left in place:

| Name | Owner | Status |
|---|---|---|
| oo-a366-build | a366 | Exited (1) 3 minutes ago, this review, removed later by the owned rebuild |
| oo-a353-node-smoke | unlabeled | Exited (0) about a minute ago |
| oo-a064-rust194-contract2 | a064 | Exited (0) 5 days ago |
| oo-a064-rust194-contract | a064 | Exited (101) 5 days ago |
| oo-a064-pg1710-jobs | a064 | Exited (0) 5 days ago |
| oo-a064-rust194-smoke | a064 | Exited (0) 6 days ago |

Running `oo.kind=rust-builder` count: 0. Host `cargo`/`rustc`/`rustdoc`/`rustup` count: 0. This review's running builder count at the snapshot: 0. This review's running PostgreSQL count: 1.

Session directories present from a351 upward: a351, a352, a354, a355, a356, a357, a358, a359, a360, a361, a362, a363, a364, a365, a366, a367. The a353 directory was not in the listing. Frontend work stays with a351, a353, and a352. This review did not open those trees.
