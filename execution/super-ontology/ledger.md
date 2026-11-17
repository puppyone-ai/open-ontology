# Super Ontology 台账

按 50 §9 维护：每张卡一行，每个闭环签收追加一行。状态只允许 todo、in_progress、review、merged、signed、dropped。主线程独占本文件。

| 卡号 | 闭环 | 状态 | 分支 | 提交（首 → 末） | PR | 证据 | 备注 |
|---|---|---|---|---|---|---|---|
| SO-SPEC | — | merged | feat/super-ontology (主线程) | 台账、spec 类提交 | — | — | 非卡片的主线程提交 |
| SO-B-S1-01 | L1 | merged | feat/super-ontology (主线程) | 5f1fb464e → 5f1fb464e | — | contracts/docs-api/ | docs-api 1.0-draft |
| SO-B-S1-02 | L1 | merged | feat/super-ontology (主线程) | b5b6d3831 → b5b6d3831 | — | — | oo-docs 骨架；arch 新增 FEATURE_GATED_EDGES |
| SO-B-S1-03 | L1 | merged | feat/super-ontology (主线程) | 9cedcc05c → e0c5f080f | — | — | docir、DocumentSource、context-pack schema |
| SO-B-S1-04 | L1 | merged | so/B-S1-04-golden-set | b44b1736c → 07834ceab | — | — | 51 份文档 + 4 红队 |
| SO-B-S1-05 | L1 | merged | so/B-S1-05-oo-eval-docs | ec491bdf7 → ec491bdf7 | — | — | 自带 JSON 解析器，L2 前改用 serde_json |
| SO-B-S1-06 | L1 | merged | so/B-S1-06-trailer-ledger-checks | edde0add0 → edde0add0 | — | — | — |
| SO-B-S1-07 | L1 | merged | so/B-S1-07-supply-chain + 主线程 | b8f115446 → 17fe3096d | — | — | 34 个新包；cargo deny 三条 unmaintained 公告待 owner 决定 |
| SO-B-S1-08 | L1 | merged | so/B-S1-08-private-terms | 462d12cf3 → 462d12cf3 | — | — | CI 需加密变量注入词表（owner） |
| SO-V-S1-01 | L1 | in_progress | so/V-S1-01-formal-toolchain | — | — | — | 续作中 |
| SO-V-S1-02 | L1 | todo | — | — | — | — | FM1 |
| SO-F-S1-01 | L1 | merged | codex/super-drive-F-S1-01-flags | 5b2f67d9 → 7139ed22 | — | — | Puppyone 本地 |
| SO-F-S1-02 | L1 | merged | codex/super-drive-F-S1-02-surface-shell | b99a6796 → fd692501 | — | — | Puppyone 本地 |
| SO-F-S1-03 | L1 | merged | codex/super-drive-F-S1-03-main-skeleton | 2a6e514d → 42bc8b08 | — | — | Puppyone 本地 |
| SO-F-S1-04 | L1 | merged | codex/super-drive-F-S1-04-i18n-namespace | 739ced90 → 5caedb04 | — | — | 6 种语言初译待复核 |
| SO-F-S1-05 | L1 | merged | codex/super-drive-F-S1-05-contract-vendor | 4b07b81d → da1d2a33 | — | — | 需随 3235e5c97 重新同步契约 |
| SO-F-S1-06 | L1 | in_progress | codex/super-drive-F-S1-06-arch-baseline | 8e7eeffb → 1aaa282a | — | — | flag-off T1 已真实运行通过；flag-on T1 待做 |
| SO-F-S1-07 | L1 | review | — | — | — | execution/super-ontology/puppy-issues-pointer/README.md | puppy-issues 为独立远端仓库：草稿在 OO 内维护，随 Puppyone 最终 PR 一并发布（主线程决定，2026-11-17） |
| SO-B-S2-01 | L2 | merged | so/B-S2-01-docs-migrations + 主线程 | b813bef4f → 648b093f3 | — | — | 迁移 0137–0140 由主线程提交；容器测试主线程复跑 8/8 |
| SO-B-S2-02 | L2 | merged | so/B-S2-02-objectstore-fs | 1fc956757 → 8ccfae090 | — | — | 无 libc，O_NOFOLLOW 以逐级 symlink_metadata + dev/ino 比对代替；残留窄 TOCTOU |
| SO-B-S2-03 | L2 | merged | so/B-S2-03-cas | 16fd8f50f → 16fd8f50f | — | — | — |
| SO-B-S2-04 | L2 | merged | so/B-S2-04-job-framework | 03313816c → 9afc868ef | — | — | jobs_live 主线程复跑 10/10；抖动哈希暂用 SHA-256 |
| SO-B-S2-05 | L2 | merged | so/B-S2-05-ingest-reconcile | — | — | — | ingest_fs 主线程复跑 14/14 |
| SO-B-S2-06 | L2 | merged | so/B-S2-06-md-txt-ir | 347c4d956 → 3df429b14 | — | — | GB18030 待 encoding_rs 准入；normalize 内有 canonical_json 副本待换 re-export |
| SO-B-S2-07 | L2 | merged | so/B-S2-07-rules-publish | e100e3d03 → 35c9c4e10 | — | — | 无模型 md/txt 子集：实体 F1 0.609、事实 F1 0.214（基线，非验收） |
| SO-B-S2-08 | L2 | merged | so/B-S2-08-compile-minimal | 2b580b857 → 761c5ddb5 | — | — | 红队真实 docx 待 L4；AIGC 首行在 front matter 前 |
| SO-B-S2-09 | L2 | merged | so/B-S2-09-control-api, so/B-S2-09-pipeline-wiring | — → 032017cf9 | — | evidence/L2/ | 流水线接线已合入 |
| SO-B-S2-10 | L2 | merged | so/B-S2-10-oo-serve | → f431223c2 | — | evidence/L2/ | L2 简化：引擎为宿主子进程，compose 路径随 B-S3-01 |
| SO-B-S2-11 | L2 | merged | so/B-S2-11-context-mount | → 376714a94 | — | evidence/L2/ | revoke/list/refs 待 B-S6 |
| SO-B-S2-12 | L2 | merged | so/B-S2-12-l2-e2e | → db950b4cc | — | evidence/L2/ | 验收 1–5（4 为 API 层）37/37，主线程复跑 37/37 |
| SO-F-S2-01 | L2 | todo | — | — | — | — | — |
| SO-F-S2-02 | L2 | todo | — | — | — | — | — |
| SO-F-S2-03 | L2 | todo | — | — | — | — | — |
| SO-F-S2-04 | L2 | todo | — | — | — | — | — |
| SO-F-S2-05 | L2 | todo | — | — | — | — | — |
| SO-F-S2-06 | L2 | todo | — | — | — | — | — |
| SO-F-S2-07 | L2 | todo | — | — | — | — | — |
| SO-F-S2-08 | L2 | todo | — | — | — | — | — |
| SO-F-S2-09 | L2 | todo | — | — | — | — | — |
| SO-F-S2-10 | L2 | todo | — | — | — | — | — |
| SO-F-S2-11 | L2 | todo | — | — | — | — | — |
| SO-F-S2-12 | L2 | todo | — | — | — | — | — |
| SO-F-S2-13 | L2 | todo | — | — | — | — | — |
| SO-V-S2-01 | L2 | todo | — | — | — | — | — |
| SO-V-S2-02 | L2 | todo | — | — | — | — | — |
| SO-V-S2-03 | L2 | todo | — | — | — | — | — |

## 签收

| 闭环 | 签收提交 | OO sha | Puppyone sha | 证据 | 日期 |
|---|---|---|---|---|---|
