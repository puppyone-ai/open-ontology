# Product stack

`oo-product-stack` is the preflight, health, journey, and input-SBOM oracle. `ops/oo.sh` only deploys that binary and the compose project.

Dev is localprocess. It is not production and it does not use runsc. Standard and minimal fail closed when `runsc` is absent. They do not fall back to runc.

Source control and release control stay on recipe profiles until they have a locked commit. Selfbuild is not started while its producer binaries are absent. No service is mocked and no ready bit is written by hand.

G1 owns `apps/web/deploy/**`. This directory does not change those files. The identity pin G1 already records stays `a6f92da8a0bf872f8a30efb116c2c7b14463505b`.

See `RUNBOOK.md` for the command order and `G1-HANDOFF.md` for the web deploy boundary.
