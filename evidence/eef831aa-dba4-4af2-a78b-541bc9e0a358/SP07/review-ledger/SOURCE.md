# SOURCE

Product commit `f862683945a6e5a8803e3f9450d0f51cde5a98ad`. Nineteen files, 6712 insertions. `Cargo.lock` is 50228 bytes, lock version 4. No root `Cargo.toml` or root `Cargo.lock` change.

SHA-256 of the committed tree:

```
0a853e7efd4d15857170ece2abf3f311f4596e7c9b73b3579c23d89c3d56524d  tools/review-ledger/CONTRACT.md
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
f8e3d3230b4774bac84d26a77f80847129caab2c5eb9095d61326c06e384391c  tools/review-ledger/src/store.rs
55f2a832859edf0b8686bec38b9f1b0eea4a065d925cec719439176217b1fbd2  tools/review-ledger/src/tls.rs
c4a569326f41144beafe0fd45ac8f42a2d5ef2911557d08ad8fe9084704414b1  tools/review-ledger/tests/contract.rs
26158a7159044a52ef156038930314b5f30b5104f3f6982ec96757e3a674637b  tools/review-ledger/tests/live.rs
65f865a7d4c311965e79c692b3ca742bf21302254507d2ec2fdd4bbb47738ec8  tools/review-ledger/tests/support/world.rs
```

Pins in `Cargo.toml`: edition 2024, rust-version 1.94.0, serde 1.0.229, serde_json 1.0.151, tokio 1.53.1, rustls 0.23.45, tokio-rustls 0.26.5, tokio-postgres 0.7.18, postgres-types 0.2.14, postgres-protocol 0.6.12, tokio-postgres-rustls 0.14.0. `description-pack` and `description-review` are path dependencies at `=0.1.0`. `unsafe_code` is forbidden.
