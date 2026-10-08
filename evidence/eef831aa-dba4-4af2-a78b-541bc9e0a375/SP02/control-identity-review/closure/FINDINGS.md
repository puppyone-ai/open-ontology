# Findings

## F1. Closed, then accepted inside APPROVE_LIMITED

Historical withhold, kept as written in the parent `FINDINGS.md`: on `35bd6b8157cecd89f2ae085b012b4b0137562ddb`, OIDC Core 1.0 §3.1.3.7 failed. `aud=rp-client` returned 401 `audience.rejected` and `aud=configured-aud` returned 302. `binding_holds` was false. Proof hash `f917a652b0fcec042699852ea2a3d43d109ae3c5e40a3952f184eebb8ae82438`.

Replacement proof on `a6f92da8a0bf872f8a30efb116c2c7b14463505b`: the same split rejects `other-aud` and `configured-aud`, accepts `aud=rp-client` with and without `azp`, accepts multiple audiences only when `azp=rp-client`, and rejects multiple audiences when `azp` is absent. Proof hash `5d748b03a1598c8cd7a94b4be4846f2adfa38a74065a7017ee02f8cfcd4829df`. Verdict text `FINDING_CLOSED`.

That closure is now part of **APPROVE_LIMITED original E54**. The parent withhold files were not edited.

## F2. Retained note, unchanged

`audit_event` owner `oo_identity_admin`. Trigger `BEFORE DELETE OR UPDATE`. It does not cover `TRUNCATE`. The table owner can `TRUNCATE`. The service SQL does not. This closure did not run `TRUNCATE`. The note does not withhold the child.

## No new withhold

The five-file diff changes the `aud` membership comparison and the contract sentence that describes it. The other original checks stay on the 68-case packet because their code is outside the diff, or because that packet set `audience` equal to `client_id`, where the old and new comparisons are the same predicate. The matrix is in `REPORT.md`.
