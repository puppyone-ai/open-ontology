# Findings index

Verdict **REQUEST_CHANGES** on `13ad15b4bc66979b96a2604c20e2ca4b20dd92f7`.

- RC-1. Prose pack digest `c4f5c00c…` is not in raw JSON. `pack.sha256` is the incomplete digest `5cba6c68…`.
- RC-2. Cancel id `b_369d1e63…` and its S9 `awaiting` claim are prose. Green JSON has `canceled` and pack 403 without the id.
- RC-3. Row counts and `request_sha256` are not in the green summary. READY-CHECK cited the cards hash while `ready` was false.
- RC-4. Shop negative is `generate.failed`. Producer unit 401/timeout/`ready` false is not a shop event.

`b_f36a05605491fe53429435921539158c` is not full S10. OC-01 through OC-06 are not approved. Details are in the evidence FINDINGS.md.
