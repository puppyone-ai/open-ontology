# G1 handoff

G1 owns `apps/web/deploy/**`, including `README.md` and `APIBINDING.md`. This product stack does not edit those files.

The fixed identity build G1 already points at remains commit `a6f92da8a0bf872f8a30efb116c2c7b14463505b`. Runtime origin stays unset. `oidc_unbound` stays as G1 recorded it. No web asset, route, or deploy file was written by UUID `eef831aa-dba4-4af2-a78b-541bc9e0a395`.

Backend compose lives only in `deploy/product-stack/`. It is not a replacement for the G1 web deploy output.
