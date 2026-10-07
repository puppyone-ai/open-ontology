# Driver review

Reviewer: this session, after the focused Linux run and before the live chain. Scope is the frozen contract and `testing/real-db/pg-e2e`. This review does not approve the live chain, M1-I01, M1-I02, or a parent unit.

## Result

The driver matches the frozen contract. Focused evidence `linux/focused` passed on rustc 1.94.0: `passed=9 failed=0 ignored=0`, Clippy `-D warnings` exit 0, container `1c1105e70cae4a1dfa2a594f547a958fbeb3110e9912ebf3bfbc85f96f544d56` removed, 6 GiB, 1 CPU, cap-drop ALL, no pull. Earlier compile and assertion failures are kept under `linux/focused-attempt-1` through `focused-attempt-5`.

## Contract checks

Row formulas, the 100000-row total, generation 317, the identity preimage, and the abc SHA-256 vector are in the Rust crate. `national_id` is omitted from decisions. Wide and narrow visibility match the contract lists. Embedded statement exits, stderr, and per-statement `max_rows` match the contract table. `amount` is not in a statement whose expected exit is 0. The numeric probe records the real exit.

Inspect and apply call `pg-onboarding`. Fresh reads call `query-pg`. Physical COPY SQL is built from `import-result.json` physical names. The comparison is line-for-line against `line_at`. The product `canonical_digest` is copied into the measurement file and is not the gold. Non-claims asserted on both outcomes include parent acceptance, development-unit and component approval, M1-I01, M1-I02, semantic publication, authentication, authorization, natural language, harness mount, public auth, and config identity.

The onboarding request still omits a budget. The driver records `budget_defaults` from the manifest. It does not raise `import_ms` above the tool maximum of 180000.

## Not claimed

A passing focused run is not a PostgreSQL result. The live chain has not been executed at the time of this review.
