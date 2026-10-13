# DL01-W1-41 MongoDB driver preflight

Preparation probe for a separate Rust process. It is not `oo.source.mongodb/v1`, not source acceptance, and not a DBX-W13 acceptance.

The driver pin is the official `mongodb` crate 3.3.0, Apache-2.0, with `compat-3-0-0` and `rustls-tls`. The server fixture is MongoDB 8.0.32 Community under the SSPL, plaintext, with no Enterprise audit. `NOTICE.md` and `PIN.json` record that split. `rustls-tls` satisfies the crate's compile requirement. The fixture does not turn TLS on.

Identity split and profiler level 2 follow `testing/dl01-targets/mongodb/scripts` at `b24610ca75a7f9aa5d245a171aac1fa681409c55`. Database, collection, account, and document values in this directory belong to `oo-dl01w1-41`.

`scripts/run-all.sh` admits one Rust 1.94 builder and one private MongoDB 8.0.32 container. The probe and decoder are prebuilt binaries. The decoder crate does not depend on `mongodb`. Promotion of `batch.arrow` happens only after `scripts/oracle.py` exits 0.
