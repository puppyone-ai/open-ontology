# DL01-W1-33 operator-preview fixed review

Decision: `approve`. Approved scope: the frozen operator-preview child.

Parent SP09-W02, DL01, source-support, a real Builder chain, and a 100k run stay unaccepted.

Prior review `318eb50f61853d9a85a0f079aeaf4d4b0f75e95d` is preserved. This review covers the binding repair at source `f2357a026033fbac5beb20b4bf8555d808881fe7` and seal `7d1ca1c1eda67299b6833b298829036f96a830f9`.

The sealed role pack for `oo_r_reader` lists `public.batch_c` columns `id`, `qty`, `amount`, and `note`. It omits `secret`. Pack digest `836e69bb74e115aa37204d52e47a66ad08fc0db65cb645e57bfe4d7795d5d5c8`. After `GRANT SELECT` on the physical `secret` column, a policy that adds `secret` and runs `SELECT secret FROM public.batch_c` exits 9, class `pack_mismatch`, execution `8f0ca1ad-eac8-44db-9542-d2e1c40a4184`. Audit directory `audit-wide` was not created. Stdout has no `rows` array and does not contain `w33-sealed-drawer-token`.

`describe` binds `knowledge.dataset` and `draft_pack.dataset` to `public.batch_c`. Knowledge columns are `amount`, `id`, `note`, and `qty`, each on `public.batch_c`. The string `preview.sample` does not appear. A policy whose table is `preview.sample` exits 9 before its audit directory exists. `SELECT id FROM preview.sample` against the bound `public.batch_c` policy exits 6, class `guard`, with no rows.

The positive query returns the independent fixture under execution `32fd7bd3-6e06-404a-b624-5f947555bb86`.

`draft_pack.admitted_fields` still enumerates sealed candidates `public.batch_a`, `public.batch_b`, and `public.batch_c`. Each field carries its own dataset. The bound dataset and the returned columns are `public.batch_c`. That listing is an observation in FINDINGS.md.

Evidence: `evidence/dfabd828-74fa-45e6-8c0a-f3131ec21baa/DL01/operator-preview-fixed-review/`.
Decisive stdout: `linux/phases/secret-widen.stdout`.
