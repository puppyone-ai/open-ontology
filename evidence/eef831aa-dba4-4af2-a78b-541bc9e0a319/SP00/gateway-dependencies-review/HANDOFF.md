# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a319`. Read `CONTRACT.md`, `REPORT.md`, and `FINDINGS.md`. The verdict is **APPROVE_LIMITED** for the exact dependency baseline in the fixed archive `52ce234c81f2e56e2f05abd43e3ce70ec5ac5a96`.

Passing proof: `linux/preoverlay-2/`, container `23920f1c098b793580f5ca3891dc15ac61463314b1be412e05ac0ee2567579b9`, `inside_ok=2026-10-07T11:32:15Z`, four loopback tests passed in 0.36s, Clippy exit 0. Failed oracle attempt: `linux/preoverlay/`, container `d713a1e5f7d388316fd850f13fc8abf9892420250213bfe9c81de3f9538128ad`, exit 1. Both stay.

The decision file stays `UNAPPROVED` on disk. This handoff does not accept a parent goal, a release, or a public Gateway API. `oo-gateway` does not link these pins. PostgreSQL cancellation on timeout is not implemented. No JWT or OIDC crate was added.

## What the next consumer card has to do

That card is a new task and a separate approval. On the one crate that will serve HTTP, set `axum`, `tower`, and `tower-http` to `workspace = true` and run a targeted fetch. The root lock may gain only these registry triples:

- axum 0.8.9 `31b698c5f9a010f6573133b09e0de5408834d0c82f8d7475a89fc1867a71cd90`
- axum-core 0.5.6 `08c78f31d7b1291f7ee735c1c6780ccde7785daae9a9206026862dab7d8792d1`
- httpdate 1.0.3 `df3b46402a9d5adb4c86a0cf463f42e19994e3ee891101b1841f30a545cb49a9`
- matchit 0.8.4 `47e1ffaa40ddd1f3ed91f717a33c8c0ee23fff369e3aa8772b9605cc1d22f4c3`
- mime 0.3.17 `6877bb514081ee2a7ff5ef9de3281f14a4dd4bceac4c09388074a6b5df8a139a`

No version bump and no checksum change of any package already in the lock. Do not blanket-update the workspace. Do not copy the probe lock's dependency arrays over the root lock. The probe is a smaller feature set: its hyper 1.11.1 list adds `httpdate` and omits `h2` and `want`. A consumer that still needs the current hyper edges must keep them and add the edges the pinned features require. Show that dependency-array diff explicitly. `linux/preoverlay-2/edges.txt` is the evidence of which arrays moved in the probe, not a patch.

Use `GlobalConcurrencyLimitLayer` when one permit must cover every route. Use `TimeoutLayer::with_status_code` for HTTP 408. Leave query, form, json, and tower-http `auth` off unless a later contract adds them. Connector manifests stay outside this card.

## Builder notes

Image digest and limits are in `REPORT.md`. Private volumes were `oo-a319-eef831aa-cargo` and `oo-a319-eef831aa-target`. Network was `oo-a319-eef831aa-net`. `CLEANUP.txt` is the removal list. No prune.

`SHA256SUMS` is written after this file and after both proof host tails. It skips itself only.
