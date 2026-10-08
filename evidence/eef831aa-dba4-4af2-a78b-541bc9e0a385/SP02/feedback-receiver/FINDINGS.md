# Findings

Session `eef831aa-dba4-4af2-a78b-541bc9e0a385`. Limited to the feedback receiver queue slice that was proved. Not a parent finding, not full SP02-W06, and not full SP13-W04.

## Fixed before the passing run

- Binary path. `env!("CARGO_BIN_EXE_feedback_receiver")` is unset. The package binary is `feedback-receiver`, so Cargo exports `CARGO_BIN_EXE_feedback-receiver`. The live test reads that name first.
- Prior-id borrow. The rules test passed a slice that moved the expected candidate id and then read it. The passing test uses `std::slice::from_ref`.
- Published-row probe. `$3::jsonb` made the prepared parameter type jsonb. A Rust string cannot serialize that type, so `Error::code()` was empty and the check never ran. The probe now sends text and casts `($3::text)::jsonb`, which is the same cast the server path uses for stored JSON. The passing run got SQLSTATE `23514` from `receipt_not_published`.
- Clippy `-D warnings` on `tests/rules.rs`: replace the cloned one-element slice, assert the overlap bool with `assert!`, and collapse the SQL/question walk into one let-chain.

## What the passing run showed

- Accept, byte-identical replay, restart replay, conflict 409, cross-project 403, body-project 403, oversized 400, 33-item bound 400, stranger URI 403, and `POST /v1/admin` 405.
- Shop and other certificates each stored one receipt. The admin snapshot after the test was `shop=1` and `other=1`.
- The injection string is stored as untrusted feedback text. The metric SQL stayed `SELECT sum(amount) FROM logistics.shipment`.
- `UPDATE`, `DELETE`, `TRUNCATE`, and `SELECT rolpassword FROM pg_authid` returned `42501`. `oo_r_reviewer` was absent. Cross-database `CONNECT` failed both ways.
- The seeded child lock matched root registry checksums. `cargo generate-lockfile` did not run.

## Left as bounds of this slice

- Full SP02-W06 still includes the collector and the gateway WAL end-to-end. Those are the a371 seam and the SP06 follow-up named in the plan. This process does not write `oo_audit`.
- Full SP13-W04 still includes the suggestion job, the review page, and the G-gate after a person accepts a candidate. This slice stops at queryable candidates and sources. Accepting a candidate here does not grant a role and does not publish.
- `knowledge_verified` stays false because no Gateway config is supplied to `produce`.
- `rust-toolchain.toml` remains channel 1.98.1. The proof sets `RUSTUP_TOOLCHAIN=1.94.0` and does not edit that file.
- `deploy/trusted-demo/pg/pg-wrap.sh` still comments an older hash than the file on disk (`05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a`). The recipe was not edited.
- `raw/live-result.txt` and `raw/console.log` append. The passing file still contains the attempt-3 prefix above the passing notes.
- Independent parent acceptance is still open. This card does not sign it.
