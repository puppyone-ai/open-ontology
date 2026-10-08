# Findings

## Corrected — ID Token `aud` contains `client_id`

a375 withheld original E54 because `claims` in `src/crypto.rs` accepted an ID token when any `aud` entry equaled the configured audience. With `audience=configured-aud` and `client_id=rp-client`, `aud=rp-client` was `audience.rejected` and `aud=configured-aud` opened a session.

Commit `a6f92da8a0bf872f8a30efb116c2c7b14463505b` checks that `aud` contains `client_id`. `azp`, when present, must still equal `client_id`. More than one audience still requires `azp`. A single audience equal to `client_id` is accepted when `azp` is absent. The config field `audience` is still parsed and still enters `config_commit`. It is not the membership check.

The live fixture keeps `audience=oo-control` and `client_id=oo-control-client`. Signed private HTTPS results are in `REPORT.md` and `linux/raw/author-proof.json`: client_id accepted, arbitrary and configured-only rejected, multi-audience without `azp` rejected, multi-audience with matching `azp` accepted, single matching `aud` without `azp` accepted.

## Unchanged wire

These hashes match the 089b publication recorded by the a370 packet:

- `openapi/control-identity.openapi.yaml` `d49bec8cd3ef59bce78e8f8123fd8ea58be7bedac6d84108a13a2c197bd0f040`
- `schema/config.schema.json` `1c593466dd680c58684f7ddc5bd40134a1b1152ca383f354d21d14fb70415681`
- `schema/error.schema.json` `1a6725385580d9d1ec668f84f71d6c441a05fcbe54d07b88046bfdaf98d26238`
- `schema/current.schema.json` `ffd32c86bd4f5188fb08adcbba49d46231c9cadaaf8530031b5055922e7823cb`
- `schema/bootstrap-consume.schema.json` `dcbbe3c79f86b14e621c17c340a470011763d4c8ee1698c5fc59f21759831697`
- `schema/bootstrap-result.schema.json` `42da8ab161dbf673d748b9e9c519c2af2ad75892c2bdc41f89f7bda407748e0d`
- `schema/logout.schema.json` `3c66428d1c325d45d8d96fcde5e8d1ac9cd53531eecf7c4932f2e6744cfc2064`
- `schema/capability-matrix.json` `c2122752ad5138f134527c88a761a03484fefe9b30364f28b60c30298ea118fd`

`CONTRACT.md` is now `e8589e5c6cdec584657443a4b485ba2ecdf375b097eb6f3076388f6b8f79ceb5`. Error code `audience.rejected` is unchanged. No frontend file and no `tools/selfbuild-control-api` file was written.

## Retained note

a375 recorded that the `audit_event` owner can `TRUNCATE` because the trigger is `BEFORE DELETE OR UPDATE`. This repair did not run `TRUNCATE` and did not open a database campaign. The service SQL does not issue `TRUNCATE`.

## Not accepted

Independent review of `a6f92da8a0bf872f8a30efb116c2c7b14463505b` has not been run. The reused 68-case packet is the measurement of `35bd6b815`, not a review of this commit. Customer IdP integration is not claimed.
