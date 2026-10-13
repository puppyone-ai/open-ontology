# Findings

Verdict **REQUEST_CHANGES** on `13ad15b4bc66979b96a2604c20e2ca4b20dd92f7`.

- RC-1 bound. Both success pack files hash to `c4f5c00c…`. Envelope `output_sha256` of those bytes is `b3483d9e…`.
- RC-2 build id bound as directory `b_369d1e63bae4faa084ffc0c3474dd4a1`. The GET stage list is still absent.
- RC-3 bound. Cards evidence `2123011bec…`, schema `eae55fc9…`, reltuples party/account/entry `2000`/`8000`/`90000`, three request hashes in the evidence `FINDINGS.md`.
- RC-4. Retained generations show HTTP 200. No 401 or timeout refusal with `ready` false was on disk.

Full S0–S10 scope stays unapproved. OC-01 through OC-06 are not inherited.
