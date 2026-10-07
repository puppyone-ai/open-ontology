# SOURCE

Amendment product commit `6b3a92c2e06a2e91a74393b739ec8f1c565ae17b`. Five files changed, 332 insertions, 8 deletions. `Cargo.lock` is 50228 bytes, lock version 4, SHA-256 unchanged. Migration SHA-256 unchanged. No root `Cargo.toml` or root `Cargo.lock` change.

SHA-256 of the tree at that commit:

```
97482b294485af9a4c10988e81dae39bc0680ed2adf2466d8ab7438577cc0e88  tools/review-ledger/CONTRACT.md
476abb33b767a16b395756c05172d7ba39c8ac2b64f4f6d284b026de1663877d  tools/review-ledger/Cargo.lock
ea4c83ccbbe2fab9646169fea8f22392b8858df45d3b1d84314fedf1d20120a5  tools/review-ledger/Cargo.toml
75e79573642a90b0b3d8f7704b5b080704656f7d9494b056162b9bc0dd63843c  tools/review-ledger/migrations/oo_review_ledger_v1.sql
ce97c4ca3ea690b5bde77958499390651cd5287d53ebbe21e0fe0f649a039207  tools/review-ledger/src/canonical.rs
39c2f9c9e42993cb65283c1521250de4bbdd4ec4929308bc6b30ee067a812889  tools/review-ledger/src/compiler.rs
2beb66dc385a31cf0e7045d0073ca1bbbf4d700f54b8101a42f1411650d12e90  tools/review-ledger/src/control.rs
f70f6e6c02e0f8ea1d8d5255fea7ea6bd70a67fc14571f308db2a063542c3053  tools/review-ledger/src/error.rs
8f98e57c23d134f073389a9861a435d66522cd1cafd1ca30d1a1f8f385391669  tools/review-ledger/src/fields.rs
f343d6c825a350ae8bd3c248536ca054f9e363426384fbc100ee0bf3eb1a6772  tools/review-ledger/src/lib.rs
17bed22fd797fbe245572cb0c31d254feacc7520294a9b4550d7ce498f6dda43  tools/review-ledger/src/main.rs
c247af8693f27c758062466dede73dbd9870aac974ac900201ae3b92beab02fc  tools/review-ledger/src/model.rs
eac61aa8d9d0eebe935be0d438b6877b40845656601c4c5a8180edd97963f037  tools/review-ledger/src/sha256.rs
777d007d039835bdfea3062e339eb7931036504f3494ff29a458242bfa289d2f  tools/review-ledger/src/sql.rs
2b6fe281e243aa1c18c36d59e3667316a2a59b45fb4df43fc467336386742ec5  tools/review-ledger/src/store.rs
55f2a832859edf0b8686bec38b9f1b0eea4a065d925cec719439176217b1fbd2  tools/review-ledger/src/tls.rs
3dc1485ffd8aed79d537e32c4ea9cee3ca2a5f32403f9dae34603f21d7718c92  tools/review-ledger/tests/contract.rs
f297fd3027d1d09149fd0e692aca5a294083291efdd93a74dff0223c764497e5  tools/review-ledger/tests/live.rs
6e342cc4fd737b4a06c4d81b608e56f9983f990d865dd5d33238b4ddb1d9ae93  tools/review-ledger/tests/support/world.rs
```

Files whose bytes changed from `f862683945a6e5a8803e3f9450d0f51cde5a98ad`: `CONTRACT.md`, `src/store.rs`, `tests/contract.rs`, `tests/live.rs`, `tests/support/world.rs`.

Pins in `Cargo.toml`: edition 2024, rust-version 1.94.0, serde 1.0.229, serde_json 1.0.151, tokio 1.53.1, rustls 0.23.45, tokio-rustls 0.26.5, tokio-postgres 0.7.18, postgres-types 0.2.14, postgres-protocol 0.6.12, tokio-postgres-rustls 0.14.0. `description-pack` and `description-review` are path dependencies at `=0.1.0`. `unsafe_code` is forbidden.
