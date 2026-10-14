# Doris handoff, 2026-10-14

Owner `5106fed0-5bcd-4aaf-8df7-838c95a7d7e2`. Worker `doris-connection-whole`. Source id `dorispreview`.

## Run

- Run directory: `~/.cache/oo-source-onboarding/runs/e875c8b248464b3f90acafcae65ac52d` mode 0700.
- State: `recipe_completed`. Proof: `whole.json` SHA-256 `dfc4ddd8490a4e579b50eb6840e99af560706f6f96f790be6d5beca741873ea4`.
- Engine registry: doris `whole` true, same proof. Other engine rows were left as they were.
- Six-field input: `/tmp/oo-doris-connection-whole/six-field.json`. Secret ref `secret://doris-connection-whole/reader`. The secret file stays mode 0600 and is not in git.
- Do not resume this run. Shared `product_flow.py` no longer matches the recipe pin.

## Resources still up

- `oo-doris-connection-whole-pg` database `dorisconnection2`
- `oo-doris-connection-whole-api` on the existing TLS listener
- `oo-doris-connection-whole-gateway` listen `127.0.0.1:18573`, binding generation 10
- Source FE `oo-dl01w1-43-ns2-fe` at `172.16.143.10:9030`, database `oo_dl01w1_43`
- Token file `/tmp/oo-doris-connection-whole/gateway/client.token` mode 0600. Plaintext is not recorded here.
- Failed journals, DB volumes, and the reader/loader/admission ELFs are retained.

## Pending

Fixed review and coordinator acceptance stay owner-pending. This handoff does not publish the project and does not push.
