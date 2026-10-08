# Frozen interface for G1 and G2

Contract `oo.selfbuild-control-api/v1` in `tools/selfbuild-control-api/`.
OpenAPI `tools/selfbuild-control-api/openapi/selfbuild-control-api.openapi.yaml`.

This file records the operation IDs. It does not bind the frontend SDK. G1 owns that binding. Operation IDs, OpenAPI, JSON schemas, and the migration stayed frozen through the shop run.

| operationId | Group | Method and path |
|---|---|---|
| `createBuild` | B04 | `POST /control/v1/builds` |
| `listBuilds` | B04 | `GET /control/v1/builds` |
| `getBuild` | B04 | `GET /control/v1/builds/{build_id}` |
| `cancelBuild` | B04 | `POST /control/v1/builds/{build_id}/cancel` |
| `getBuildCards` | B04 | `GET /control/v1/builds/{build_id}/cards` |
| `getBuildCandidates` | B04 | `GET /control/v1/builds/{build_id}/candidates` |
| `listReviewItems` | B04 | `GET /control/v1/reviews` |
| `getReviewItem` | B04 | `GET /control/v1/reviews/{item_id}` |
| `previewReviewBatch` | B04 | `POST /control/v1/reviews/preview` |
| `decideReviewItem` | B04 | `POST /control/v1/reviews/{item_id}/decision` |
| `listClassifications` | B04 | `GET /control/v1/classifications` |
| `previewClassifications` | B04 | `POST /control/v1/classifications/preview` |
| `confirmClassifications` | B04 | `POST /control/v1/classifications/confirm` |
| `compileBuildPack` | B05 | `POST /control/v1/builds/{build_id}/pack` |
| `getBuildPack` | B05 | `GET /control/v1/builds/{build_id}/pack` |

Session cookie is `__Host-oo-session`. Mutations send `Origin` and `X-OO-CSRF`. `identity_revision` is the integer from `getIdentityCurrent`. This API does not issue that cookie.

`compiled_pack` is `not_published`. `published` and `active` are false. Classification does not grant.

Identity contract and schemas stay `089b00d7f70da1bc42f7e201dfa070748c651d5b`. The shop run called the fixed identity service `a6f92da8a0bf872f8a30efb116c2c7b14463505b`. That implementation is independently accepted. This run does not claim a customer IdP certificate. A missing identity server still returns `503` `dependency.not_ready`. G1 operation IDs in this file are unchanged.
