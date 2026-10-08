# Handoff

Author repair is commit `a6f92da8a0bf872f8a30efb116c2c7b14463505b`. Original E54 is not accepted. The SP02 parent and the full goal are not accepted. The next step is an independent review of this commit.

## Contract readers

Read-only consumers keep the 089b schema and OpenAPI bytes. Those bytes are unchanged, and `audience.rejected` is unchanged. The ID Token rule they should read is the corrected sentence in `tools/control-identity/CONTRACT.md` at this commit:

`iss` exact, `aud` contains the registered `client_id` for every permitted configuration, a distinct configured audience does not replace `client_id`, multiple audiences require `azp` equal to `client_id`, a present `azp` equals `client_id`, and a single audience equal to `client_id` is accepted when `azp` is absent

Config files still carry `audience`. A value distinct from `client_id` still loads and still participates in `config_commit`. It does not satisfy `aud`. Frontend and `tools/selfbuild-control-api` were not modified.

## Evidence

- UUID `eef831aa-dba4-4af2-a78b-541bc9e0a379`.
- `REPORT.md`, `FINDINGS.md`, this file, `CONTRACT-SENTENCE.md`.
- `linux/raw/smoke.exit` 0 before the edit. `cargo-test-focused.exit` 0. `cargo-clippy.exit` 0. `run-proof.exit` 0.
- `linux/raw/FAILED-ATTEMPTS.md` records no failed proof run.
- Reused, not rerun: a375 `independent-proof.json` `f917a652b0fcec042699852ea2a3d43d109ae3c5e40a3952f184eebb8ae82438` (68 cases, 0 failed on `35bd6b815`).
- `linux/raw/owned-cleanup.txt`. Owned containers, volumes, the network, and the `/tmp` secret directory are gone. `oo-a373-pg` was left running.

## Scope

Writes are `tools/control-identity/**` in the product commit and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a379/SP02/control-identity-audience-repair/**` in the evidence commit. No root manifest, no lock update, no new denominator.
