# Findings

## F1. Closed on `a6f92da8a0bf872f8a30efb116c2c7b14463505b`

The original withhold was OIDC Core 1.0 §3.1.3.7: `aud` must contain the relying party's `client_id`. On `35bd6b8157cecd89f2ae085b012b4b0137562ddb`, with `audience` `configured-aud` and `client_id` `rp-client`, `aud=rp-client` returned 401 `audience.rejected` and `aud=configured-aud` returned 302. `binding_holds` was false. Evidence: the parent `linux/raw/independent-proof.json` hash `f917a652b0fcec042699852ea2a3d43d109ae3c5e40a3952f184eebb8ae82438`.

This recheck archived `a6f92da8a0bf872f8a30efb116c2c7b14463505b` and drove a private HTTPS RS256 issuer. The same split was:

- `aud=other-aud`, `azp=rp-client`: HTTP 401 `audience.rejected`
- `aud=rp-client`, `azp=rp-client`: HTTP 302, then `GET /control/v1/identity/current` 200 for subject `fixture-admin`
- `aud=configured-aud`, `azp=rp-client`: HTTP 401 `audience.rejected`

Single `aud=rp-client` with `azp` absent returned 302 and current 200. `aud` `["rp-client","extra"]` with `azp=rp-client` returned 302 and current 200. The same multiple audiences with `azp` absent returned 401 `audience.rejected`.

`linux/raw/audience-proof.json` records `original_finding` `closed` and `azp_rules` `held`. The signature check runs before the audience check. JWKS was fetched once for the six legs.

## Not accepted

Original E54 stays unaccepted. The 68-case packet was not rerun on this commit. Customer IdP, the SP02 parent, and the full goal stay unaccepted.

## Retained note

The parent packet's table-owner `TRUNCATE` note is unchanged. This recheck did not run `TRUNCATE` and did not query `audit_event` ownership.
