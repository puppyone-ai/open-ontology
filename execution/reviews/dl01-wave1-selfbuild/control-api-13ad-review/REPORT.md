# S0–S10 review index

Product `13ad15b4bc66979b96a2604c20e2ca4b20dd92f7`. Verdict: **REQUEST_CHANGES**. The full S0–S10 product scope is not approved. Reconciliation `09da5e17` / `d7bccd02` is not this review.

Full record: `evidence/8a00aded-d6ea-43ea-ab3e-683533508f88/DL01/selfbuild-fixed/control-api-13ad-review/`.

Success job `b_172745443da950f62320bdd1e2cec895` has a preserved awaiting-to-succeeded transition in `failed-s5-second-structural/workflow-success.json` and a later resume in the green JSON. Historical job `b_f36a05605491fe53429435921539158c` stays `running` at stage column S5 with S9 `awaiting`. Failure job `b_c8b4a42a5484fd235a0a9a7e9e6bd88f` stays S5 `generate.failed`.

RC-1 through RC-4 are the missing raw bindings: success pack digest, cancel job id and stages, cards counts and `request_sha256`, and any already-retained model 401 or timeout excerpt. No new model run was started.
