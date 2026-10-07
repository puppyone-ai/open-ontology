# Review 2. Lock diff

Reviewed commit `773c461104729d6c289eae8640c7c2a28207c447`. The Rust `lockdiff` tool compiled from that commit's `tools/lockdiff.rs` inside container `83cb5ecac838c956bc82054f1219002d8804e3720eb7e693707e9b4eb6d51897`. Inputs were the archived root `Cargo.lock`, the archived `probe/Cargo.lock`, and the archived `upstream/new-crate-pins.txt`. As-of time `2026-10-06T12:47:28Z`.

`linux/review-rerun/lockdiff_subset.exit` is 0. stdout:

```
added=5
axum 0.8.9 31b698c5f9a010f6573133b09e0de5408834d0c82f8d7475a89fc1867a71cd90
axum-core 0.5.6 08c78f31d7b1291f7ee735c1c6780ccde7785daae9a9206026862dab7d8792d1
httpdate 1.0.3 df3b46402a9d5adb4c86a0cf463f42e19994e3ee891101b1841f30a545cb49a9
matchit 0.8.4 47e1ffaa40ddd1f3ed91f717a33c8c0ee23fff369e3aa8772b9605cc1d22f4c3
mime 0.3.17 6877bb514081ee2a7ff5ef9de3281f14a4dd4bceac4c09388074a6b5df8a139a
changed=0
denied=0
subset=pass
```

Removed registry triples: none. Changed registry triples: none. Denied names: none.

The archived root lock SHA-256 is `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a` (`lock.exit` 0). `lockdiff exact` of that file against itself is `lockdiff_exact_self.exit` 0. That self-compare only shows the parser accepts the file. The unchanged-baseline claim is the SHA-256 and the subset result `changed=0`.

`lockdiff-selfcheck.stdout` is `selfcheck=pass`.

This review does not set the baseline to approved.
