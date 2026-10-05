# Review

Same session as the implementation. Frozen scope only: `reviewed_effective.rs`, the `lib.rs` export, `reviewed-effective-oos`, `tests/reviewed_effective.rs`, and `contracts/builder/reviewed-effective-oos-v1.md`. This is not an independent review and it does not accept M1-D18A.

## Checked against the green run

The command reads one `oo.builder.reviewed-effective-oos-command/v1` object. An argument exits 2. Scanner failures keep the scanner code. `protocol.mismatch`, `field.unknown`, and `settings.invalid` write no stdout and do not echo the owner string. Those cases passed in `usage_scanner_and_settings_failures_do_not_echo_the_body`.

The seal recomputes the captured draft. Tampering with an embedded `pii` fails `seal.rejected` before a class can apply. The library test and the contract agree.

Decisions are explicit. Duplicate, invalid PII, unknown name, stale fact, and a renamed catalog hash are refused or hidden. `auto_accept` does not classify `id`. Generated and curated hashes differ on the empty curated echo. `decisions_sha256` matches `canonical_sha256` of the decisions array in the library test.

`compile_role_pack` consumes the effective document. Clerk, peek, and the ledger closure probe behave as the contract describes. Hidden `email` stays on the dataset until a structural reject. After reject, the existing merge removes it, the decision is `decision.unknown`, and the role pack reports `visibility.unknown_column`. The contract was corrected after overlay attempt 2. That correction matches `merge_effective`.

Provenance and the role-pack manifest do not claim authentication, grants, or publication.

## Left as specified

A rejected column that remains on an effective dataset is covered by a direct `apply_decisions` test. The structure-draft compiler does not emit that shape today. A rejected relationship is removed by the same merge and is not restored.

`Cargo.toml` was not updated. The description still says the crate does not classify.

The green Clippy command is `--lib --bins --test reviewed_effective`. It does not lint the other `oo-builder` test binaries. The pre-edit smoke ran `captured_pg`, `review_pg_cli`, `role_pack`, and `role_pack_cli` on the fixed archive before the overlay.

No PostgreSQL was used. No dependency file changed. M1-D18 publication, authentication, and grants stay open.
