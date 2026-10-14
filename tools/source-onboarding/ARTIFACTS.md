# Fixed artifacts for source-owned whole recipes

Read this index from the shared entry. Do not copy ELF bytes or another source's code into a recipe. Pin the paths below. Secret files stay in the caller's 0700 directory and 0600 refs; they are not listed here.

Host check on 2026-10-14: all eight sha256 values match the files. `docker info` did not return; no container was added and nothing was rebuilt. Linux retest of this tree was not run.

Protocol `oo.fixed-runtime-artifacts/v1`. Source index: `/tmp/oo-pg46-runtime-finish/artifact-index.json`. API source commit `d34813ee44ec558460ee571ef9f37f1ecdd1e026`. Shared-entry base `3a8950483`. Private runtime template, mode 0600, do not commit: `/tmp/oo-star-runtime-finish/runtime.json`.

| name | path | sha256 | source |
| --- | --- | --- | --- |
| worker | `/tmp/oo-pg46-runtime-finish/job-worker-successor` | `8f22c09bad5c24a9ca7229d7dc73729f7416e9e62c6320fef5db644ba31fb29d` | `5038a03b5` |
| loader | `/tmp/oo-pg46-runtime-finish/oo-pipeline` | `2ff14f824c6d4391e6487de6dd133e190966039ec77ce2345aad9cae1b1188e4` | `15ea6cb07` |
| admission | `/tmp/oo-pg46-runtime-finish/continuous/oo-source-admission` | `07e7651a98b4ef76582f67666e1245065546db204a50899554981619efc05dbf` | |
| capture | `/tmp/oo-pg46-runtime-finish/continuous/postgres-preview-fixed` | `1e567472e06ef81c7514dfb1554658ac9af258983fe28a2fafd11ffa2cb330af` | `753dd2de4` |
| api | `/tmp/oo-pg46-runtime-finish/selfbuild-control-api-observation-fixed` | `025aceded6cc922c0a10cb2e9d535ccc433ad6f082e5c1c3b86f4797cafc50ae` | `d34813ee44ec558460ee571ef9f37f1ecdd1e026` |
| pg_builder | `/tmp/oo-pg46-runtime-finish/continuous/pg-builder-current` | `58e5eed3d88f22c2b27c758b7fb064258188eb09ce379e74eac66c5af741a91d` | |
| oo_builder | `/tmp/oo-pg46-runtime-finish/continuous/oo-builder` | `aea0d3433bdece2cfc0c2b84075c9d43ac84b767603979d5f4842be2af78a0f5` | |
| classic_bootstrap | `evidence/ad13411e-d04e-4c91-8a20-49caf867aa23/DL01/postgres-product/linux/product/oo-pipeline-f80af746` | `f80af74646087fb39470a55b7eaefc81ccbf2a28269e2d7ff137f67a4d9cfc19` | |

Whole registration also pins `tools/source-onboarding/product_flow.py` and `tools/agent-product-verifier/dialogue.py` by their current bytes. A metadata recipe does not use this index to claim whole.
