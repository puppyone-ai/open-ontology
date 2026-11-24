# Super Ontology 台账

按 50 §9 维护：每张卡一行，每个闭环签收追加一行。状态只允许 todo、in_progress、review、merged、signed、dropped。主线程独占本文件。

| 卡号 | 闭环 | 状态 | 分支 | 提交（首 → 末） | PR | 证据 | 备注 |
|---|---|---|---|---|---|---|---|
| SO-SIGN-L2 | L2 | signed | feat/super-ontology (主线程) | 签收提交 | — | testing/super-ontology/evidence/L2/ | OO 侧（D80 阶段一）；标签 super-ontology/L2（本地） |
| SO-SIGN-L8 | L8 | signed | feat/super-ontology (主线程) | 签收提交 | — | testing/super-ontology/evidence/L8/ | OO 侧证据已记录，未签收：conformance_share 偶发失败（向量开启全量运行 1 次，单独通过）；download 因 S3 镜像不在本机被阻塞；L8 自身验收项全部通过；pgvector 基础镜像法务审阅（D67）与无 Docker 冷下载为 owner 事项 |
| SO-SIGN-L7 | L7 | review | so/sign-v2 | 证据提交（未签收） | — | testing/super-ontology/evidence/L7/ | OO 侧证据已记录，未签收：真实 OSS/COS/R2 桶与云队列需 owner 凭证（验收 1 未做）；MNS 签名、COS 事件格式未确认 |
| SO-SIGN-L5 | L5 | signed | so/sign-v2 | 签收提交 | — | testing/super-ontology/evidence/L5/ | OO 侧（D80 阶段一）；独立复核 42/42；修订模式视图与 editor 写入在阶段二；标签 super-ontology/L5（本地） |
| SO-SIGN-L3 | L3 | signed | feat/super-ontology (主线程) | 签收提交 | — | testing/super-ontology/evidence/L3/ | OO 侧证据已记录，未签收：S3 格子（RustFS、MinIO）、download、s3_rustfs 未独立重跑（固定镜像不在本机，拉取需 owner 批准） |
| SO-SIGN-L1 | L1 | signed | feat/super-ontology (主线程) | 签收提交 | — | testing/super-ontology/evidence/L1/ | 标签 super-ontology/L1（本地） |
| SO-SIGN-L4 | L4 | signed | feat/super-ontology (主线程) | 签收提交 | — | testing/super-ontology/evidence/L4/ | OO 侧证据已记录，未签收：conformance_share（FM6）在全量 live 套件负载下于 ff1d2ad02 失败 1 次（单独通过），根因修复进行中；CLI 验收 58/58 |
| SO-SIGN-L6 | L6 | signed | feat/super-ontology (主线程) | 签收提交 | — | testing/super-ontology/evidence/L6/ | OO 侧（D80 阶段一）；独立复核 CLI 36/36、e2e 28/28；ASR 选型与许可记录于 supply-chain/media-asr-model-reviews.json；macOS 宿主机解码与 puppyone share 在阶段二；标签 super-ontology/L6（本地） |
| SO-SPEC | — | merged | feat/super-ontology (主线程) | 台账、spec 类提交 | — | — | 非卡片的主线程提交 |
| SO-B-S1-01 | L1 | signed | feat/super-ontology (主线程) | 5f1fb464e → 5f1fb464e | — | contracts/docs-api/ | docs-api 1.0-draft |
| SO-B-S1-02 | L1 | signed | feat/super-ontology (主线程) | b5b6d3831 → b5b6d3831 | — | — | oo-docs 骨架；arch 新增 FEATURE_GATED_EDGES |
| SO-B-S1-03 | L1 | signed | feat/super-ontology (主线程) | 9cedcc05c → e0c5f080f | — | — | docir、DocumentSource、context-pack schema |
| SO-B-S1-04 | L1 | signed | so/B-S1-04-golden-set | b44b1736c → 07834ceab | — | — | 51 份文档 + 4 红队 |
| SO-B-S1-05 | L1 | signed | so/B-S1-05-oo-eval-docs | ec491bdf7 → ec491bdf7 | — | — | 自带 JSON 解析器，L2 前改用 serde_json |
| SO-B-S1-06 | L1 | signed | so/B-S1-06-trailer-ledger-checks | edde0add0 → edde0add0 | — | — | — |
| SO-B-S1-07 | L1 | signed | so/B-S1-07-supply-chain + 主线程 | b8f115446 → 17fe3096d | — | — | 34 个新包；cargo deny 三条 unmaintained 公告待 owner 决定 |
| SO-B-S1-08 | L1 | signed | so/B-S1-08-private-terms | 462d12cf3 → 462d12cf3 | — | — | CI 需加密变量注入词表（owner） |
| SO-V-S1-01 | L1 | signed | so/V-S1-01-formal-toolchain + 主线程 | 94d00f929 → 429b0e0d9 | — | formal/TOOLCHAIN.md | CI 按 (模型, 不变式) 分片 |
| SO-V-S1-02 | L1 | signed | so/V-S1-02-fm1-jobs-model | 4ecdfd968 → 88d60b3b8 | — | formal/reviews/FM1.md | 独立评审：带修改通过；两项阻塞随 B-S3-04 修复 |
| SO-F-S1-01 | L1 | signed | codex/super-drive-F-S1-01-flags | 5b2f67d9 → 7139ed22 | — | — | Puppyone 本地 |
| SO-F-S1-02 | L1 | signed | codex/super-drive-F-S1-02-surface-shell | b99a6796 → fd692501 | — | — | Puppyone 本地 |
| SO-F-S1-03 | L1 | signed | codex/super-drive-F-S1-03-main-skeleton | 2a6e514d → 42bc8b08 | — | — | Puppyone 本地 |
| SO-F-S1-04 | L1 | signed | codex/super-drive-F-S1-04-i18n-namespace | 739ced90 → 5caedb04 | — | — | 6 种语言初译待复核 |
| SO-F-S1-05 | L1 | signed | codex/super-drive-F-S1-05-contract-vendor | 4b07b81d → da1d2a33 | — | — | 需随 3235e5c97 重新同步契约 |
| SO-F-S1-06 | L1 | signed | codex/super-drive-F-S1-06-arch-baseline | 8e7eeffb → 1aaa282a | — | — | flag-off T1 已真实运行通过；flag-on T1 待做 |
| SO-F-S1-07 | L1 | review | — | — | — | execution/super-ontology/puppy-issues-pointer/README.md | puppy-issues 为独立远端仓库：草稿在 OO 内维护，随 Puppyone 最终 PR 一并发布（主线程决定，2026-11-17） |；草稿已随 L1 签收（OO af7100afa，Plan SO-SPEC），Puppyone 侧不提交（D7、D43），发布随最终 PR
| SO-B-S2-01 | L2 | signed | so/B-S2-01-docs-migrations + 主线程 | b813bef4f → 648b093f3 | — | — | 迁移 0137–0140 由主线程提交；容器测试主线程复跑 8/8 |
| SO-B-S2-02 | L2 | signed | so/B-S2-02-objectstore-fs | 1fc956757 → 8ccfae090 | — | — | 无 libc，O_NOFOLLOW 以逐级 symlink_metadata + dev/ino 比对代替；残留窄 TOCTOU |
| SO-B-S2-03 | L2 | signed | so/B-S2-03-cas | 16fd8f50f → 16fd8f50f | — | — | — |
| SO-B-S2-04 | L2 | signed | so/B-S2-04-job-framework | 03313816c → 9afc868ef | — | — | jobs_live 主线程复跑 10/10；抖动哈希暂用 SHA-256 |
| SO-B-S2-05 | L2 | signed | so/B-S2-05-ingest-reconcile | — | — | — | ingest_fs 主线程复跑 14/14 |
| SO-B-S2-06 | L2 | signed | so/B-S2-06-md-txt-ir | 347c4d956 → 3df429b14 | — | — | GB18030 待 encoding_rs 准入；normalize 内有 canonical_json 副本待换 re-export |
| SO-B-S2-07 | L2 | signed | so/B-S2-07-rules-publish | e100e3d03 → 35c9c4e10 | — | — | 无模型 md/txt 子集：实体 F1 0.609、事实 F1 0.214（基线，非验收） |
| SO-B-S2-08 | L2 | signed | so/B-S2-08-compile-minimal | 2b580b857 → 761c5ddb5 | — | — | 红队真实 docx 待 L4；AIGC 首行在 front matter 前 |
| SO-B-S2-09 | L2 | signed | so/B-S2-09-control-api, so/B-S2-09-pipeline-wiring | — → 032017cf9 | — | evidence/L2/ | 流水线接线已合入 |
| SO-B-S2-10 | L2 | signed | so/B-S2-10-oo-serve, so/B-S2-10b-compose-runtime | → d73948ca1 | — | evidence/L2/ | compose 运行时 41/41；默认仍为宿主进程；仅 arm64 镜像 |
| SO-B-S2-11 | L2 | signed | so/B-S2-11-context-mount | → 376714a94 | — | evidence/L2/ | revoke/list/refs 待 B-S6 |
| SO-B-S2-12 | L2 | signed | so/B-S2-12-l2-e2e | → db950b4cc | — | evidence/L2/ | 验收 1–5（4 为 API 层）37/37，主线程复跑 37/37 |
| SO-F-S2-01 | L2 | merged | codex/super-drive-F-S2-01-* | — | — | — | Puppyone 本地；集成分支 npm test 6227/6227 |
| SO-F-S2-02 | L2 | merged | codex/super-drive-F-S2-02-* | — | — | — | Puppyone 本地；集成分支 npm test 6227/6227 |
| SO-F-S2-03 | L2 | merged | codex/super-drive-F-S2-03-* | — | — | — | Puppyone 本地；集成分支 npm test 6227/6227 |
| SO-F-S2-04 | L2 | merged | codex/super-drive-F-S2-04-* | — | — | — | Puppyone 本地；集成分支 npm test 6227/6227 |
| SO-F-S2-05 | L2 | merged | codex/super-drive-F-S2-05-* | — | — | — | Puppyone 本地；集成分支 npm test 6227/6227 |
| SO-F-S2-06 | L2 | merged | codex/super-drive-F-S2-06-* | — | — | — | Puppyone 本地；集成分支 npm test 6227/6227 |
| SO-F-S2-07 | L2 | merged | codex/super-drive-F-S2-07-* | — | — | — | Puppyone 本地；集成分支 npm test 6227/6227 |
| SO-F-S2-08 | L2 | merged | codex/super-drive-F-S2-08-* | — | — | — | 真引擎 T1 15 项通过；--native 待 Terminal 复跑；集成分支 npm test 6329/6329 |
| SO-F-S2-09 | L2 | in_progress | codex/super-drive-F-T2-01-acceptance-flags | 561ef041 → 3d6f5804 | — | — | 验收构建开 Super Drive 已完成；T2 运行器写好，Peekaboo 需屏幕录制权限；按 D80 顺延到阶段二 |
| SO-F-S2-10 | L2 | merged | codex/super-drive-F-S2-10-* | — | — | — | 验收渠道打包成功（1m49s）；集成分支 npm test 6329/6329 |
| SO-F-S2-11 | L2 | merged | codex/super-drive-F-S2-11-* | — | — | — | 无项目入口；集成分支 npm test 6329/6329 |
| SO-F-S2-12 | L2 | merged | codex/super-drive-F-S2-12-* | — | — | — | Puppyone 本地；集成分支 npm test 6227/6227 |
| SO-F-S2-13 | L2 | merged | codex/super-drive-F-S2-13-* | — | — | — | Puppyone 本地；集成分支 npm test 6227/6227 |
| SO-V-S2-01 | L2 | signed | so/V-S2-01-* | — | — | formal/ | 10,000 条轨迹与真实 SQL 一致 |
| SO-V-S2-02 | L2 | signed | so/V-S2-02-* | — | — | formal/ | FM2(md)、FM4 无反例；渲染层 FM4 回放待前端 |
| SO-V-S2-03 | L2 | todo | — | — | — | — | — |
| SO-B-S3-01 | L3 | merged | so/B-S3-01-docling-sidecar, so/B-S3-01b-opencv-nofft, so/B-S3-01c-notices | → 681ded0a4 | — | evidence/L3/ | OpenCV 自编无 FFmpeg，表格结构已开（TableFormer 许可待律师 L5）；THIRD_PARTY_NOTICES；SBOM PASS；无出网实测 |
| SO-B-S3-02 | L3 | merged | so/B-S3-02-xlsx-csv | — | — | evidence/L3/ | xlsx/csv 38/38 锚点；隐藏行列未跳过 |
| SO-B-S3-03 | L3 | merged | so/B-S3-02-xlsx-csv | — | — | evidence/L3/ | 嗅探、zipguard、加密检测、CSV |
| SO-B-S3-05 | L3 | merged | so/B-S3-01-docling-sidecar | — | — | evidence/L3/ | pdf 7/7、docx 10/10、pptx 6/6；docx 段落序号 201/201 |
| SO-B-S3-06 | L3 | merged | so/B-S3-01-docling-sidecar | — | — | evidence/L3/ | sidecar 监督 6/6 |
| SO-B-S3-04 | L3 | merged | so/B-S3-04-state-machine | — | — | — | 完整状态机、毒丸隔离、FM1 修复（0138 解析上限隔离） |
| SO-B-S3-07 | L3 | merged | so/B-S3-07-files-api | — | — | — | 文件详情、blocks、content、批量重试、tree 汇总 |
| SO-B-S3-09 | L3 | merged | so/B-S3-09-freshness-latency | — | — | — | 新鲜度七态、延迟埋点；测试隔离修复见 Fixes |
| SO-B-S4-05 | L3 | merged | so/B-S4-05-doctor-obs | — | — | — | doctor docs、指标、磁盘守卫、日志脱敏；mounts 未覆盖 |
| SO-F-S3-01 | L3 | merged | codex/super-drive-F-S3-01-* | — | — | — | 上传队列 |
| SO-F-S3-02 | L3 | merged | codex/super-drive-F-S3-02-* | — | — | — | 状态矩阵、106 个错误码 8 语言 |
| SO-F-S3-03 | L3 | merged | codex/super-drive-F-S3-03-* | — | — | — | 选择、横幅、批量重试 |
| SO-F-S3-04 | L3 | merged | codex/super-drive-F-S3-04-* | — | — | — | 详情栏解析结果；FilePreview 嵌入与 PDF #page 未做 |
| SO-B-S5-01 | L4 | merged | 主线程 | — | — | evidence/L4/ | 迁移 0141 |
| SO-B-S5-13 | L4 | merged | 主线程 | — | — | evidence/L4/ | 迁移 0142（share 侧表随 S6） |
| SO-B-S5-02 | L4 | merged | so/B-S5-02-rules-extension | — | — | evidence/L4/ | 全格式规则 |
| SO-B-S5-04 | L4 | merged | so/B-S5-04-resolve | — | — | evidence/L4/ | 别名、身份、合并、拆分 |
| SO-B-S5-05 | L4 | merged | so/B-S5-05-temporal | — | — | evidence/L4/ | 双时间与取代 |
| SO-B-S5-06 | L4 | merged | so/B-S5-06-review-publish | — | — | evidence/L4/ | 队列；按来源 100 条一次接受 |
| SO-B-S5-07 | L4 | merged | so/B-S5-06-review-publish | — | — | evidence/L4/ | DOC-G1–G5；接受到新版本 1.73 s |
| SO-B-S5-08 | L4 | merged | so/B-S5-06-review-publish | — | — | evidence/L4/ | 实体与 resolve API |
| SO-B-S5-10 | L4 | merged | so/B-S5-10-consistency | — | — | evidence/L4/ | drive_rev、哈希链 |
| SO-B-S5-11 | L4 | merged | so/B-S5-10-consistency | — | — | evidence/L4/ | 完整性约束 |
| SO-B-S5-12 | L4 | merged | so/B-S5-10-consistency | — | — | evidence/L4/ | 用户纠错 |
| SO-B-S5-03 | L4 | merged | so/B-S5-03-llm-supplement | — | — | evidence/L4/ | 录制响应测试；job handler 待接线 |
| SO-B-S5-09 | L4 | merged | so/B-S5-09-eval-rules-only | — | — | evidence/L4/ | 无模型：实体 F1 0.675、事实 F1 0.351（仅本地格式，下界） |
| SO-B-S3-08 | L3 | merged | so/B-S3-05-pipeline-routing | — | — | evidence/L3/ | 故障注入 9/9 |
| SO-B-S4-01 | L3 | merged | so/B-S4-01-s3-opendal | — | — | evidence/L3/ | 自建 reqwest 传输，无新包 |
| SO-B-S4-02 | L3 | merged | so/B-S4-01-s3-opendal | — | — | evidence/L3/ | RustFS/MinIO 各 20/20 |
| SO-B-S4-03 | L3 | merged | so/B-S4-01-s3-opendal | — | — | evidence/L3/ | 200 MiB 13.2 s，内存 +32.8 MiB |
| SO-B-S4-04 | L3 | merged | so/B-S4-01-s3-opendal | — | — | evidence/L3/ | docs-rustfs、docs-minio 格子 |
| SO-B-S4-06 | L3 | merged | so/B-S3-05-pipeline-routing | — | — | evidence/L3/ | 1000 文件 23.8 min |
| SO-V-S3-01 | L3 | merged | so/FV-laneA-integrated | — | — | evidence/L3/ | FM2 全量 + KP7 |
| SO-V-S3-02 | L3 | merged | so/FV-laneA-integrated | — | — | evidence/L3/ | FM3 fs；s3 部分未覆盖 |
| SO-B-S6-01 | L4 | merged | so/C-S6-compile-share | — | — | evidence/L4/ | VisibleGraph + leak guard 四层 |
| SO-B-S6-04 | L4 | merged | so/C-S6-compile-share | — | — | evidence/L4/ | Share 生命周期；漂移 0.04 s |
| SO-B-S6-05 | L4 | merged | so/C-S6-compile-share | — | — | evidence/L4/ | refs 凭证 401/403/410/429 |
| SO-B-S6-06 | L4 | merged | so/C-S6-compile-share | — | — | evidence/L4/ | oo context 全参数；CLI 对齐 |
| SO-B-S6-07 | L4 | merged | so/C-S6-compile-share, so/C-L4-finish | — | — | evidence/L4/ | 红队全格式 0 命中；重放；出处；编译 p95 0.54 s |
| SO-B-S6-08 | L4 | merged | so/C-S6-compile-share, so/C-L4-finish | — | — | evidence/L4/ | L4 脚本 71/71；CLI 验收 58/58；质量目标未达（进行中） |
| SO-B-S6-10 | L4 | merged | so/C-S6-compile-share | — | — | evidence/L4/ | 轻量漂移、D51 |
| SO-B-S6-11 | L4 | merged | so/C-S6-compile-share | — | — | evidence/L4/ | coverage、UPDATES、check；迁移 0145 |
| SO-B-S6-12 | L4 | merged | so/C-S6-compile-share | — | — | evidence/L4/ | 任务组、as_of、保留作业 |
| SO-B-S6-13 | L4 | merged | so/C-S6-compile-share | — | — | evidence/L4/ | AIGC |
| SO-V-S5-01 | L4 | merged | so/FV-laneA-L4 | — | — | evidence/L4/ | FM5 |
| SO-V-S5-02 | L4 | merged | so/FV-laneA-L4 | — | — | evidence/L4/ | FM8；待两名独立评审 |
| SO-V-S6-01 | L4 | merged | so/FV-laneA-L4 | — | — | evidence/L4/ | KP5、KP6 |
| SO-V-S6-02 | L4 | merged | so/FV-laneA-L4 | — | — | evidence/L4/ | FM6 |
| SO-V-S6-03 | L4 | merged | so/FV-laneA-L4 | — | — | evidence/L4/ | FM10、FM11；发现并修复 7 个缺陷 |
| SO-B-S9-01 | L6 | merged | so/B-S9-01-ocr-parser | — | — | evidence/L6/ | ocr-parser；扫描召回 0.965 |
| SO-B-S9-03 | L6 | merged | 主线程 | — | — | evidence/L6/ | IR v1.1 |
| SO-B-S9-04 | L6 | merged | so/B-S9-01-ocr-parser | — | — | evidence/L6/ | 内嵌图片 OCR；字符召回 ≥0.97 |
| SO-B-S10-05 | L6 | merged | so/B-S9-01-ocr-parser | — | — | evidence/L6/ | media-parser；自编 LGPL FFmpeg 无专利解码器 |
| SO-B-S10-06 | L6 | merged | so/B-S9-01-ocr-parser | — | — | evidence/L6/ | ASR 比测：建议 Paraformer-zh ONNX（cpu 档） |
| SO-B-S10-07 | L6 | merged | so/B-S9-01-ocr-parser | — | — | evidence/L6/ | 2 h 续跑复用、逐字节重放 |
| SO-B-S10-09 | L6 | merged | so/B-S9-01-ocr-parser | — | — | evidence/L6/ | 每页一个关键帧 8/8 |
| SO-B-S10-10 | L6 | merged | so/B-S9-01-ocr-parser + 主线程 | — | — | evidence/L6/ | 媒体池；迁移 0146 |
| SO-B-S9-02 | L6 | merged | so/B-L6-2-engine-wiring | — | — | evidence/L6/ | 类型升格；只有人能升格 |
| SO-B-S10-01 | L6 | merged | so/B-L6-2-engine-wiring | — | — | evidence/L6/ | refresh 差异节 |
| SO-B-S10-02 | L6 | merged | so/B-L6-2-engine-wiring | — | — | evidence/L6/ | outdated 重跑与 DOC-G10；并发 1 |
| SO-B-S10-03 | L6 | merged | so/B-L6-2-engine-wiring | — | — | evidence/L6/ | `:rebuild`（docs-api 1.2） |
| SO-B-S10-08 | L6 | merged | so/B-L6-2-engine-wiring | — | — | evidence/L6/ | 说话人身份三层 |
| SO-B-S7-01 | L5 | merged | feat/super-ontology (车道 B 分支并入) | eeb6d8ce7 → bc1e32728 | — | evidence/L5/ | docs-api 1.3 的 Suggest 接口与示例 |
| SO-B-S7-02 | L5 | merged | feat/super-ontology (车道 B 分支并入) | ce0cc707e → ce0cc707e | — | evidence/L5/ | suggest/ 目录收集与 oo docs suggestion |
| SO-B-S7-03 | L5 | merged | feat/super-ontology (车道 B 分支并入) | 2a49ef6f2 → 57c82ff4a | — | evidence/L5/ | 建议记录、四条影响规则、存储与日摘要；PG 实测 |
| SO-B-S7-04 | L5 | merged | feat/super-ontology (车道 B 分支并入) | b28181b0c → b28181b0c | — | evidence/L5/ | MCP suggest、context_check 工具与 share updates 资源 |
| SO-B-S7-05 | L5 | merged | feat/super-ontology (车道 B 分支并入) | e86e3bb08 → e86e3bb08 | — | evidence/L5/ | suggester、editor Share 的 suggest:write 作用域 |
| SO-B-S7-06 | L5 | merged | feat/super-ontology (车道 B 分支并入) | 867120bb5 → b91561b8a | — | evidence/L5/ | 代理接口列出 suggest 工具；源基线 revision 33 |
| SO-B-S8-02 | L5 | merged | feat/super-ontology (车道 B 分支并入) | 9a86e0226 → d99e761ae | — | evidence/L5/ | L5 Suggest 端到端脚本与证据 |
| SO-B-S10-04 | L6 | merged | feat/super-ontology (车道 B 分支并入) | 808238251 → 435de8415 | — | evidence/L6/ | media-parser 两处缺陷修复；L6 余项（A8、长媒体、OCR 子集、包滞后）并入 |
| SO-B-S11-01 | L7 | merged | feat/super-ontology (车道 B 分支并入) | 75b1365bb → 8c26a06a4 | — | evidence/L7/ | oss、cos、r2 Drive 走 S3 兼容路径；连接器特性开关 |
| SO-B-S11-02 | L7 | merged | feat/super-ontology (车道 B 分支并入) | 59214cb43 → ae6e895e0 | — | evidence/L7/ | 桶事件通知转变更提示；docs-api 1.4 |
| SO-B-S11-03 | L7 | merged | feat/super-ontology (车道 B 分支并入) | 53cc46cf9 → 53cc46cf9 | — | evidence/L7/ | Drive 访问策略在入库与可见图中生效 |
| SO-B-S11-04 | L7 | merged | feat/super-ontology (车道 B 分支并入) | 066a4a9c0 → 4aed48e4e | — | evidence/L7/ | 已发布文档实体投影到 Agent DB 并关联行；投影迁移 |
| SO-B-S12-01 | L7 | merged | feat/super-ontology (车道 B 分支并入) | 4c8b4c387 → 85bfedafa | — | evidence/L7/ | 云入口、共享审计、事件提示路由；审计迁移 |
| SO-B-S12-02 | L7 | merged | feat/super-ontology (车道 B 分支并入) | b075dfb88 → 4438ab7d4 | — | evidence/L7/ | --remote 挂载远端 OO serve；L7 端到端脚本 |
| SO-B-S16-01 | D83 | merged | feat/super-ontology (WP1 分支并入) | f9b853377 → 27d652733 | — | — | oo docs upload/wait 与托管本地 Drive |
| SO-B-S16-02 | D83 | merged | feat/super-ontology (WP1 分支并入) | 36d1680d9 → 36d1680d9 | — | — | oo source add/ls/show/rm/test：文件夹、桶、数据库来源，凭证 0600 引用 |
| SO-B-S16-03 | D83 | merged | feat/super-ontology (WP1 分支并入) | d4c6c7b9e → d4c6c7b9e | — | — | 原先只有 API 的命令（模型、版本、约束、审计等） |
| SO-B-S16-04 | D83 | merged | feat/super-ontology (WP1 分支并入) | da4c0943d → da4c0943d | — | — | oo serve --runtime auto|compose|embedded|host 与内嵌解析 sidecar |
| SO-B-S16-05 | D83 | merged | feat/super-ontology (WP1 分支并入) | d92c321ba → 94d0c07b0 | — | — | WP1 只用 CLI 的端到端脚本 |
| SO-B-S16-06 | D83 | merged | feat/super-ontology (WP1 分支并入) | 2fa7a799c → 7accc3958 | — | — | 数据库来源登记与 upload 自动启动 serve |
| SO-B-S16-07 | D83 | merged | feat/super-ontology (WP1b 分支并入) | 7c4b9f619 → d709b56f8 | — | — | 引擎镜像带 objectstore-s3（oo-docs-engine:dev-s3）、host.docker.internal、oo source link |
| SO-B-S18-05 | D83 | merged | feat/super-ontology (WP3b 分支并入) | 6fb345f7d → 6fb345f7d | — | — | Add docs-api 1.9 for full text search over facts and blocks |
| SO-B-S18-06 | D83 | merged | feat/super-ontology (WP3b 分支并入) | e048f1edd → e048f1edd | — | — | Serve full text search over facts and blocks from the docs engine |
| SO-B-S18-07 | D83 | merged | feat/super-ontology (WP3b 分支并入) | 80a56c236 → 80a56c236 | — | — | Search facts and blocks through the unified ontology search |
| SO-B-S18-08 | D83 | merged | feat/super-ontology (WP3b 分支并入) | eef810ea3 → eef810ea3 | — | — | Make the unified search fit the Gateway SQL guard |
| SO-B-S18-09 | D83 | merged | feat/super-ontology (WP3b 分支并入) | 0c807df78 → 6036934e9 | — | — | Run the CLI-first end to end against the real OO Gateway |
| SO-B-S16-08 | D83 | merged | feat/super-ontology (车道 F 分支并入) | f3c83b7c7 → f3c83b7c7 | — | — | Install the Docker-free document parser runtime from oo itself |
| SO-B-S16-09 | D83 | merged | feat/super-ontology (车道 F 分支并入) | 3ee274a12 → 3ee274a12 | — | — | Start the daemon from the first oo source add on a fresh HOME |
| SO-B-S16-10 | D83 | merged | feat/super-ontology (车道 F 分支并入) | 17948788a → 17948788a | — | — | Lock the rebuilt ocr-parser image with the text-layer fill |
| SO-B-S16-11 | D83 | merged | feat/super-ontology (车道 F 分支并入) | 7e8406608 → 7e8406608 | — | — | Prove docx and pdf publish from a fresh HOME without Docker |
| SO-B-S16-12 | D83 | merged | feat/super-ontology (车道 F 分支并入) | ba8dc7715 → ba8dc7715 | — | — | Test the CLI-only paths no script covered |
| SO-B-S16-13 | D83 | merged | feat/super-ontology (车道 F 分支并入) | a6c7edda3 → 1872a8155 | — | — | Record the F1 evidence and refresh the OCR subset measurement |
| SO-B-S18-01 | D83 | merged | feat/super-ontology (WP3 分支并入) | 33d794dd0 → 33d794dd0 | — | — | docs-api 1.7：统一本体查询接口 |
| SO-B-S18-02 | D83 | merged | feat/super-ontology (WP3 分支并入) | 0aac843d8 → a9535f52c | — | — | /v1/ontology/* 代理、oo ontology search/entity/query/sources、oo agent token |
| SO-B-S18-03 | D83 | merged | feat/super-ontology (WP3 分支并入) | 3036bec55 → 3036bec55 | — | — | MCP 统一工具与 oo mcp --stdio |
| SO-B-S18-04 | D83 | merged | feat/super-ontology (WP3 分支并入) | ad5a891fe → e32004408 | — | — | 只用 CLI 的端到端脚本（文档与数据库两边、负例） |
| SO-B-S17-01 | D83 | merged | feat/super-ontology (WP2 分支并入) | 52f0815cf → 07898cf22 | — | — | 投影作业：已发布文档实体与事实增量投影到共享实体命名空间，带出处 |
| SO-B-S17-02 | D83 | merged | feat/super-ontology (WP2 分支并入) | baae32550 → 2b135a8fd | — | — | ooa_ Agent 令牌、scope 校验、审计、吊销；链接路由；迁移 0150、0151 |
| SO-B-S17-03 | D83 | merged | feat/super-ontology (WP2 分支并入) | 252d1b242 → 252d1b242 | — | — | 装入器接入 0150–0152 |
| SO-B-S17-04 | D83 | merged | feat/super-ontology (WP2 分支并入) | 677689561 → 677689561 | — | — | docs-api 1.6：令牌与链接路由 |
| SO-B-S17-05 | D83 | merged | feat/super-ontology (WP2 分支并入) | db8c69d66 → 5f2b8f421 | — | — | 令牌、吊销、投影增量与跨源链接的测试 |
| SO-B-S14-02 | L8 | merged | feat/super-ontology (车道 G 分支并入) | 9f953be78 → 1a397d6a2 | — | — | Install a pinned FFmpeg-free OpenCV wheel and a pinned uv in the Docker-free runtime |
| SO-B-S14-03 | L8 | merged | feat/super-ontology (车道 G 分支并入) | b70bc1e49 → b70bc1e49 | — | — | Prove the Docker-free runtime ships no FFmpeg from a fresh HOME |
| SO-B-S14-04 | L8 | merged | feat/super-ontology (车道 G 分支并入) | 6de796563 → 6de796563 | — | — | Build the release engine with objectstore-s3 beside oo |
| SO-B-S14-05 | L8 | merged | feat/super-ontology (车道 G 分支并入) | 6235f51a3 → 6235f51a3 | — | — | Record the Docker-free distribution decisions in the ADR |
| SO-B-S14-06 | L8 | merged | feat/super-ontology (车道 G 分支并入) | af03d975b → af03d975b | — | — | Pass the pinned runtime artifacts through the WP1 CLI input run |
| SO-B-S14-07 | L8 | merged | feat/super-ontology (车道 G 分支并入) | c0bb04208 → c0bb04208 | — | — | Record the lane G WP1 and CLI-only matrix runs |
| SO-B-S15-02 | D84 | merged | feat/super-ontology (打榜车道并入) | e80b4f855 → e80b4f855 | — | testing/super-ontology/bench/ | Add the bm25/1 lexical retrieval channel to oo-docs |
| SO-B-S15-11 | D84 | merged | feat/super-ontology (打榜车道并入) | 148a598b3 → ad532b007 | — | testing/super-ontology/bench/ | Add the walk/1 anchored graph ranking channel to oo-docs |
| SO-B-S15-12 | D84 | merged | feat/super-ontology (打榜车道并入) | dffaafefb → dffaafefb | — | testing/super-ontology/bench/ | Record the verified STaRK and GraphRAG-Bench leaderboard rules |
| SO-B-S15-13 | D84 | merged | feat/super-ontology (打榜车道并入) | 54dcd666c → dc4a9f9f0 | — | testing/super-ontology/bench/ | Drive STaRK knowledge bases through the oo-docs engine |
| SO-B-S15-14 | D84 | merged | feat/super-ontology (打榜车道并入) | b78585d9d → b78585d9d | — | testing/super-ontology/bench/ | Weight graph anchors by specificity in walk/2 |
| SO-B-S15-15 | D84 | merged | feat/super-ontology (打榜车道并入) | bb7659ba0 → 80f3a9a12 | — | testing/super-ontology/bench/ | Drive GraphRAG-Bench corpora through the oo-docs engine |
| SO-B-S15-16 | D84 | merged | feat/super-ontology (打榜车道并入) | 784cd391a → 2042a4702 | — | testing/super-ontology/bench/ | Freeze the STaRK configs and record the dev runs |
| SO-B-S15-01 | D84 | merged | feat/super-ontology (打榜车道 B1 并入) | e41b983a9 → e41b983a9 | — | testing/super-ontology/bench/longmemeval-v2/ | Record the verified LongMemEval-V2 leaderboard rules |
| SO-B-S15-03 | D84 | merged | feat/super-ontology (打榜车道 B1 并入) | ea55a5761 → ea55a5761 | — | testing/super-ontology/bench/longmemeval-v2/ | Drive LongMemEval-V2 trajectories through the oo-docs engine |
| SO-B-S15-04 | D84 | merged | feat/super-ontology (打榜车道 B1 并入) | c29962c06 → c29962c06 | — | testing/super-ontology/bench/longmemeval-v2/ | Run the official LongMemEval-V2 harness with the OO memory |
| SO-B-S15-05 | D84 | merged | feat/super-ontology (打榜车道 B1 并入) | a06c3feef → a06c3feef | — | testing/super-ontology/bench/longmemeval-v2/ | Tune trajectory evidence: element ids, gaps, coverage, phrases |
| SO-B-S15-06 | D84 | merged | feat/super-ontology (打榜车道 B1 并入) | 5514a469b → 5514a469b | — | testing/super-ontology/bench/longmemeval-v2/ | Report exact-text hits and misses for quoted phrases |
| SO-B-S15-07 | D84 | merged | feat/super-ontology (打榜车道 B1 并入) | a3acbb9a7 → a3acbb9a7 | — | testing/super-ontology/bench/longmemeval-v2/ | Add the LongMemEval-V2 dev run logs |
| SO-B-S15-08 | D84 | merged | feat/super-ontology (打榜车道 B1 并入) | 1f3a7fb99 → c4dffa4eb | — | testing/super-ontology/bench/longmemeval-v2/ | Record the LongMemEval-V2 run setup, configs and dev results |
| SO-B-S16-14 | D83 | merged | feat/super-ontology (App 车道 A 并入) | 31a5a5cce → 31a5a5cce | — | apps/super-ontology-desktop/ | Embed the compose deploy files in oo and write them to the state dir on first use |
| SO-F-S19-01 | D85 | merged | feat/super-ontology (App 车道 A 并入) | f03b2ddce → f03b2ddce | — | apps/super-ontology-desktop/ | Define the preload interface of the standalone Super Ontology app |
| SO-F-S19-02 | D85 | merged | feat/super-ontology (App 车道 A 并入) | 9ddcda998 → 9ddcda998 | — | apps/super-ontology-desktop/ | Add the Super Ontology app package, build config and dependency records |
| SO-F-S19-03 | D85 | merged | feat/super-ontology (App 车道 A 并入) | f19c61800 → f19c61800 | — | apps/super-ontology-desktop/ | Add the main process, preload and engine bootstrap of the Super Ontology app |
| SO-F-S19-04 | D85 | merged | feat/super-ontology (App 车道 A 并入) | a04636b72 → a04636b72 | — | apps/super-ontology-desktop/ | Add the placeholder icon, the Apache attribution and the notice generation |
| SO-F-S19-05 | D85 | merged | feat/super-ontology (App 车道 A 并入) | 52046ddcb → 1c3bf7467 | — | apps/super-ontology-desktop/ | Add the owner-triggered workflow that builds the Super Ontology DMG |
| SO-F-S19-06 | D85 | merged | feat/super-ontology (App 车道 A 并入) | 20d53a49b → dff4fd2a5 | — | apps/super-ontology-desktop/ | Bundle the release oo with its docs engine in the Super Ontology app |
| SO-F-S19-21 | D85 | merged | feat/super-ontology (App 车道 C 并入) | 8e3494dcc → d3a42896d | — | apps/super-ontology-desktop/ | Generate the app contract modules straight from contracts |
| SO-F-S19-22 | D85 | merged | feat/super-ontology (App 车道 C 并入) | f5509afc1 → f5509afc1 | — | apps/super-ontology-desktop/ | Port the main-process and contract test suites with the fake engine |
| SO-F-S19-23 | D85 | merged | feat/super-ontology (App 车道 C 并入) | a6ba188d0 → a6ba188d0 | — | apps/super-ontology-desktop/ | Port the renderer data and service layer unit suites |
| SO-F-S19-24 | D85 | merged | feat/super-ontology (App 车道 C 并入) | da1bc380b → 1e9d8778c | — | apps/super-ontology-desktop/ | List the test ids the renderer and the T1 scenarios agree on |
| SO-F-S19-10 | D85 | merged | feat/super-ontology (App 车道并入) | 988b6bd3a → 988b6bd3a | — | apps/super-ontology-desktop/ | Add the Super Ontology design system package |
| SO-F-S19-11 | D85 | merged | feat/super-ontology (App 车道并入) | a5bfd1b30 → a5bfd1b30 | — | apps/super-ontology-desktop/ | Add the Super Ontology app data and service layer |
| SO-F-S19-12 | D85 | merged | feat/super-ontology (App 车道并入) | 7317f0bee → 7317f0bee | — | apps/super-ontology-desktop/ | Add the app shell and the Home screen |
| SO-F-S19-13 | D85 | merged | feat/super-ontology (App 车道并入) | 365879373 → 365879373 | — | apps/super-ontology-desktop/ | Add the Sources screen |
| SO-F-S19-14 | D85 | merged | feat/super-ontology (App 车道并入) | 937adc269 → 937adc269 | — | apps/super-ontology-desktop/ | Add the Ontology and Agents screens |
| SO-F-S19-15 | D85 | merged | feat/super-ontology (App 车道并入) | ee00909f2 → ee00909f2 | — | apps/super-ontology-desktop/ | Add Settings and the five-step first run |
| SO-F-S19-32 | D85 | merged | feat/super-ontology (App 车道并入) | 603864050 → 603864050 | — | apps/super-ontology-desktop/ | Wire the preload channels into the renderer and carry the test ids on its screens |
| SO-F-S19-33 | D85 | merged | feat/super-ontology (App 车道并入) | 9c0a3a7f1 → 9c0a3a7f1 | — | apps/super-ontology-desktop/ | Polish the screens after the review of the real window |
| SO-F-S19-34 | D85 | merged | feat/super-ontology (App 车道并入) | 16806e48a → 70006ebca | — | apps/super-ontology-desktop/ | Refresh the third-party notice for the renderer dependencies |
| SO-F-S19-41 | D86 | merged | feat/super-ontology (App 车道 E 并入) | 1bfecdc63 → 1bfecdc63 | — | apps/super-ontology-desktop/ | Fix stale Home counts, add database sources, eight-language catalog and whole-pixel layout |
| SO-F-S19-44 | D86 | merged | feat/super-ontology (App 车道 E 并入) | ecc31cb1c → ecc31cb1c | — | apps/super-ontology-desktop/ | Leave database sources to the connector lane and keep the first run steady |
| SO-F-S19-45 | D86 | merged | feat/super-ontology (App 车道 E 并入) | a08ab8d75 → a08ab8d75 | — | apps/super-ontology-desktop/ | Fix the T2 source seed key and the T1 scenario list |
| SO-F-S19-46 | D86 | merged | feat/super-ontology (App 车道 E 并入) | f38d97ff1 → f38d97ff1 | — | apps/super-ontology-desktop/ | Make T1 fail when a scenario fails, and add the database and visual suites |
| SO-F-S19-47 | D86 | merged | feat/super-ontology (App 车道 E 并入) | 0c5597384 → 8e11e396b | — | apps/super-ontology-desktop/ | Record the app's npm dependencies and list every bundled package in the notice |
| SO-F-S19-60 | D88 | merged | feat/super-ontology (App 车道 G 并入) | f114d4eec → 6d460bd0b | — | apps/super-ontology-desktop/ | Add the living ontology atlas to the app (D88) |
| SO-F-S19-61 | D88 | merged | feat/super-ontology (App 车道 G 并入) | 659931736 → 659931736 | — | apps/super-ontology-desktop/ | Refresh the third-party notice for the layout dependency |
| SO-B-S15-09 | D84 | merged | feat/super-ontology (打榜车道 B1 并入) | b725f39a0 → b725f39a0 | — | testing/super-ontology/bench/longmemeval-v2/ | Show complete, compact trees for distinct pages |
| SO-B-S15-10 | D84 | merged | feat/super-ontology (打榜车道 B1 并入) | 1fb9fcfbf → 1fb9fcfbf | — | testing/super-ontology/bench/longmemeval-v2/ | Retrieve every page a question compares |
| SO-F-S19-62 | D88 | merged | feat/super-ontology (App 车道 G 并入) | d8133ccc1 → d8133ccc1 | — | apps/super-ontology-desktop/ | Give the atlas one calm palette, quieter links, clearer labels and a source-only split |
| SO-B-S19-50 | D88 | merged | feat/super-ontology (App 车道并入) | 689c65ab2 → 689c65ab2 | — | apps/super-ontology-desktop/ | Accept every doctor-supported database kind in oo source add and test all resolved address |
| SO-F-S19-50 | D88 | merged | feat/super-ontology (App 车道并入) | 7865e9e28 → 7865e9e28 | — | apps/super-ontology-desktop/ | Add the CLI parity bridge: database sources, pipeline jobs, Gateway queries, diagnostics |
| SO-F-S19-51 | D88 | merged | feat/super-ontology (App 车道并入) | 25739291f → 25739291f | — | apps/super-ontology-desktop/ | Add database sources, Query, ontology YAML files and Diagnostics to the app |
| SO-F-S19-52 | D88 | merged | feat/super-ontology (App 车道并入) | 774454468 → 774454468 | — | apps/super-ontology-desktop/ | Translate the CLI parity strings into the six other app languages |
| SO-F-S19-53 | D88 | merged | feat/super-ontology (App 车道并入) | 808d92d11 → 1db4f4dcb | — | apps/super-ontology-desktop/ | Add T1 scenarios, visual tests and the parity matrix for the CLI parity screens |
| SO-F-S19-54 | D88 | merged | feat/super-ontology (App 车道并入) | cd7fef7ae → cd7fef7ae | — | apps/super-ontology-desktop/ | Repair the merge with the atlas: Ontology tabs, app state, T1 order and test ids |
| SO-F-S19-63 | D88 | merged | feat/super-ontology (App 车道并入) | 416fbe9f2 → 416fbe9f2 | — | apps/super-ontology-desktop/ | Keep group labels clear of members and tell the kinds of source apart |
| SO-B-S18-10 | D88 | merged | feat/super-ontology (车道 L 并入) | 62b045de4 → 62b045de4 | — | — | Add GET /v1/ontology/schema and oo ontology schema |
| SO-B-S18-11 | D88 | merged | feat/super-ontology (车道 L 并入) | 35d1387db → 7f981aadc | — | — | Test the ontology schema route on the real Gateway |
| SO-F-S19-70 | D88 | merged | feat/super-ontology (车道 L 并入) | a79b50c3c → a79b50c3c | — | — | Read the atlas database side from the ontology schema channel |
| SO-F-S19-71 | D88 | merged | feat/super-ontology (车道 L 并入) | a87417266 → a87417266 | — | — | Make the atlas T1 settle on the fake and on the real engine |
| SO-B-S19-51 | D88 | merged | feat/super-ontology (车道 K 并入) | 78b13505e → 78b13505e | — | — | Probe the MySQL-wire, analytic and openGauss families in oo doctor |
| SO-B-S19-52 | D88 | merged | feat/super-ontology (车道 K 并入) | 70fa36e02 → 70fa36e02 | — | — | Add oo source add for every remaining database kind |
| SO-B-S19-53 | D88 | merged | feat/super-ontology (车道 K 并入) | b82047b6a → b82047b6a | — | — | Keep the kind listing for tests only |
| SO-B-S19-54 | D88 | merged | feat/super-ontology (车道 K 并入) | 832613e8b → 3ecd9cafa | — | — | Pin the locally built arm64 lite runtime image |
| SO-B-S19-56 | D88 | merged | feat/super-ontology (车道 K 并入) | fe79e269a → fe79e269a | — | — | Scale the Builder model-call budget with the schema |
| SO-B-S19-57 | D88 | merged | feat/super-ontology (车道 K 并入) | f1e5c9ff2 → f1e5c9ff2 | — | — | Let a build review file carry the one confirmation and its edge decisions |
| SO-B-S19-58 | D88 | merged | feat/super-ontology (车道 K 并入) | 25299227f → 25299227f | — | — | Accept a non-stream reply that the gateway labels as an event stream |
| SO-B-S19-59 | D88 | merged | feat/super-ontology (车道 K 并入) | aca2cb95c → aca2cb95c | — | — | Escape the quotes in the build review help text |
| SO-B-S19-60 | D88 | merged | feat/super-ontology (车道 K 并入) | 3c5c38cbc → 3c5c38cbc | — | — | Export the owner's reviewed ontology as YAML |
| SO-B-S19-61 | D88 | merged | feat/super-ontology (车道 K 并入) | 5c3016949 → 5c3016949 | — | — | Let a build leave the owner's CLI profiles alone |
| SO-B-S19-62 | D88 | merged | feat/super-ontology (车道 K 并入) | 55e66c236 → 55e66c236 | — | — | Make the build review test write private files and format the review module |
| SO-B-S19-63 | D88 | merged | feat/super-ontology (车道 K 并入) | 5be0c92dc → 5be0c92dc | — | — | Say what is wrong when a review file cannot be read |
| SO-B-S19-64 | D88 | merged | feat/super-ontology (车道 K 并入) | 940485148 → 940485148 | — | — | Test the review file error messages |
| SO-B-S19-65 | D88 | merged | feat/super-ontology (车道 K 并入) | bd9f21ad8 → bd9f21ad8 | — | — | Pin the arm64 lite runtime built from the integration tip |
| SO-F-S19-80 | D85 | merged | feat/super-ontology (App 车道 M 并入) | 0fdb3d52d → e2e669bd1 | — | apps/super-ontology-desktop/ | Make the T2 runner address the launched process and the native open panel |
| SO-B-S19-66 | D88 | merged | feat/super-ontology (车道 K 并入) | e5c1b1c0f → e5c1b1c0f | — | — | Show why the load, candidate and builder stages failed |
| SO-B-S19-67 | D88 | merged | feat/super-ontology (车道 K 并入) | 8abadc41c → 8abadc41c | — | — | Resolve symlinked directories in file secret references |
| SO-B-S19-68 | D88 | merged | feat/super-ontology (车道 K 并入) | af0b26ddf → af0b26ddf | — | — | Strip one trailing newline from secrets |
| SO-B-S19-69 | D88 | merged | feat/super-ontology (车道 K 并入) | 23e5e90c8 → 23e5e90c8 | — | — | Say which docker command failed and why |
| SO-B-S19-70 | D88 | merged | feat/super-ontology (车道 K 并入) | 2ee16e603 → 2ee16e603 | — | — | Give the helper container the file secrets a source names |
| SO-B-S19-71 | D88 | merged | feat/super-ontology (车道 K 并入) | d0091b1bf → d0091b1bf | — | — | Keep multi-line secrets exact when stripping the newline |
| SO-B-S19-72 | D88 | merged | feat/super-ontology (车道 K 并入) | 839822527 → 839822527 | — | — | Pin the arm64 lite runtime with the secret and error fixes |
| SO-B-S20-10 | D83 | merged | feat/super-ontology (车道 V 并入) | f7640486d → f7640486d | — | — | Fix a 45-question agent QA set with gold answers before any agent run |
| SO-B-S20-11 | D83 | merged | feat/super-ontology (车道 V 并入) | cb5678a69 → cb5678a69 | — | — | Read --model as a value in oo docs model set |
| SO-B-S20-12 | D83 | merged | feat/super-ontology (车道 V 并入) | b57c2b03d → b57c2b03d | — | — | Show the Agent commands in oo ontology --help and the top-level help |
| SO-B-S20-13 | D83 | merged | feat/super-ontology (车道 V 并入) | e8b5c24c1 → e8b5c24c1 | — | — | Add a tool-calling agent harness that scores answers over the oo CLI |
| SO-B-S20-14 | D83 | merged | feat/super-ontology (车道 V 并入) | beb91e8ae → beb91e8ae | — | — | Mount host paths into the engine only for the control token |
| SO-B-S20-15 | D83 | merged | feat/super-ontology (车道 V 并入) | e8a433487 → e8a433487 | — | — | Say in the ontology help that search matches text literally |
| SO-B-S20-16 | D83 | merged | feat/super-ontology (车道 V 并入) | cdfeb39c9 → cdfeb39c9 | — | — | Tune the QA harness on the dev split and accept Gateway query ids as citations |
| SO-B-S20-17 | D83 | merged | feat/super-ontology (车道 V 并入) | 2279dd12d → 2279dd12d | — | — | Run the same agent over oo mcp --stdio in the QA harness |
| SO-B-S20-18 | D83 | merged | feat/super-ontology (车道 V 并入) | 8a49e4188 → 8a49e4188 | — | — | Pass the Gateway's message and hint through oo ontology query |
| SO-B-S20-19 | D83 | merged | feat/super-ontology (车道 V 并入) | 589b6e111 → 589b6e111 | — | — | Record the agent QA runs and the token safety checks |
| SO-B-S20-20 | D83 | merged | feat/super-ontology (车道 X 并入) | 34afb497c → 34afb497c | — | — | Mount the Agent DB secret file into the compose engine on bind |
| SO-B-S20-21 | D83 | merged | feat/super-ontology (车道 X 并入) | 2c0105740 → 2c0105740 | — | — | Deduplicate the relationships in oo ontology schema |
| SO-B-S20-22 | D83 | merged | feat/super-ontology (车道 X 并入) | d03a164e1 → d03a164e1 | — | — | Retry an OVERLOADED Gateway answer with backoff in oo serve |
| SO-B-S20-23 | D83 | merged | feat/super-ontology (车道 X 并入) | 94ac60f5f → 94ac60f5f | — | — | Ask extraction for a cross-language alias of each entity |
| SO-B-S20-24 | D83 | merged | feat/super-ontology (车道 X 并入) | e47e50476 → e47e50476 | — | — | Say how to try other forms when an ontology search is empty |
| SO-B-S20-29 | D83 | merged | feat/super-ontology (车道 X 并入，协调者合并与台账) | 293a9159d → 91e2df36f | — | — | Merge lane X and add its ledger rows |
| SO-B-S20-30 | D83 | merged | feat/super-ontology (车道 AB 并入) | b394f99c1 → b394f99c1 | — | — | Add the acceptance folder generator |
| SO-B-S20-31 | D83 | merged | feat/super-ontology (车道 AB 并入) | cf3d26659 → cf3d26659 | — | — | Add the acceptance corpus and its frozen gold |
| SO-B-S20-32 | D83 | merged | feat/super-ontology (车道 AB 并入) | 8e77219c7 → 8e77219c7 | — | — | Start the OCR and media sidecars when their locked images are here |
| SO-B-S20-33 | D83 | merged | feat/super-ontology (车道 AB 并入) | 0590b285b → 0590b285b | — | — | Give the Latin text of the corpus PDFs a real glyph width |
| SO-B-S20-34 | D83 | merged | feat/super-ontology (车道 AB 并入) | f2c198bf8 → f2c198bf8 | — | — | Decode AAC and H.264 media on the Mac without a button |
| SO-B-S20-36 | D83 | merged | feat/super-ontology (车道 AB 并入) | 1af0df4e2 → 1af0df4e2 | — | — | Render dash lines of an Ask answer as a real list |
| SO-B-S20-38 | D83 | merged | feat/super-ontology (车道 AB 并入) | f9fc8d3cc → f9fc8d3cc | — | — | Wait 180 seconds for the model's answer, not 45 |
| SO-B-S20-39 | D83 | merged | feat/super-ontology (车道 AB 并入) | 1a1ce17f3 → 66846bb8f | — | — | Record the packaged acceptance runs |
| SO-B-S20-01 | D84 | merged | feat/super-ontology (车道 U 并入) | 2e1103194 → 2e1103194 | — | — | Let the model egress wait for a reasoning model |
| SO-B-S20-02 | D84 | merged | feat/super-ontology (车道 U 并入) | 9a8e24e5d → 9a8e24e5d | — | — | Drop the about-an-Event labels the contract cannot hold |
| SO-B-S20-03 | D84 | merged | feat/super-ontology (车道 U 并入) | 19c100875 → 19c100875 | — | — | Teach extract/3 the Event, Topic, Term and tried_and_failed boundaries |
| SO-B-S20-04 | D84 | merged | feat/super-ontology (车道 U 并入) | cc1ee61e2 → cc1ee61e2 | — | — | Name the fact state row of the eval diagnostics |
| SO-B-S20-06 | D84 | merged | feat/super-ontology (车道 U 并入) | d9fac8501 → d9fac8501 | — | — | Bump the merged prompt to extract/4 |
| SO-B-S20-07 | D84 | merged | feat/super-ontology (车道 U 并入) | 1b21915aa → 1b21915aa | — | — | Record the grok-4.7 quality measurement |
| SO-B-S20-40 | D84 | merged | feat/super-ontology (车道 AD 并入) | 491e019ac → 491e019ac | — | — | Log the wire latency of each model call for a measuring run |
| SO-B-S20-41 | D84 | merged | feat/super-ontology (车道 AD 并入) | c62099442 → c62099442 | — | — | Pass the model timeout and the latency log on to the engine of the L4 run |
| SO-B-S20-42 | D84 | merged | feat/super-ontology (车道 AD 并入) | 2cf717838 → 2cf717838 | — | — | Read a bilingual header as one column |
| SO-B-S20-43 | D84 | merged | feat/super-ontology (车道 AD 并入) | 987bfa6b9 → 987bfa6b9 | — | — | Tie documents by the amounts and dates they share |
| SO-B-S20-44 | D84 | merged | feat/super-ontology (车道 AD 并入) | 04ee6f2f9 → 04ee6f2f9 | — | — | Merge the same person named in two scripts |
| SO-B-S20-45 | D84 | merged | feat/super-ontology (车道 AD 并入) | 5bcf3fb86 → 5bcf3fb86 | — | — | Publish the rules facts of a file while its model call runs |
| SO-B-S20-46 | D84 | merged | feat/super-ontology (车道 AD 并入) | 5545c1b69 → 5545c1b69 | — | — | Suggest a model when a connection is set up |
| SO-B-S20-48 | D84 | merged | feat/super-ontology (车道 AD 并入) | 7807332b9 → 7807332b9 | — | — | Skip a malformed line of the latency log |
| SO-B-S20-49 | D84 | merged | feat/super-ontology (车道 AD 并入) | bb7946512 → bb7946512 | — | — | Let the model egress admit 120 calls a minute and make it configurable |
| SO-B-S20-50 | D84 | merged | feat/super-ontology (车道 AD 并入) | 25ba983db → 25ba983db | — | — | Give the early publish its own job key and fix the cross-language tests |
| SO-B-S20-51 | D84 | merged | feat/super-ontology (车道 AD 并入) | 1fb7bcbcc → 1fb7bcbcc | — | — | Default to eight parallel model extractions and the measured model |
| SO-B-S20-52 | D84 | merged | feat/super-ontology (车道 AD 并入) | 9eea1a984 → 9eea1a984 | — | — | Lock the engine image built from this tree |
| SO-B-S20-53 | D84 | merged | feat/super-ontology (车道 AD 并入) | 3612a3ed8 → 3612a3ed8 | — | — | Record the model comparison and the acceptance run of lane AD |
| SO-B-S20-60 | D85 | merged | feat/super-ontology (车道 AE2 并入) | 3c12740e8 → 3c12740e8 | — | — | Settle an indexed file that has nothing left to publish |
| SO-B-S20-70 | D83 | merged | feat/super-ontology (车道 AF 并入) | 725b3e3a1 → 725b3e3a1 | — | — | Start oo serve without waiting for the parser install, and clean up on a signal during start-up |
| SO-B-S20-61 | D85 | merged | feat/super-ontology (车道 AE5 并入) | d923b6ac4 → d923b6ac4 | — | — | Prove that 0154 upgrades a database at 0153 with a capped Drive |
| SO-F-S19-81 | D85 | merged | feat/super-ontology (App 车道 N 并入) | 7e4fd1417 → 7e4fd1417 | — | apps/super-ontology-desktop/ | Make the T2 runner click, type and choose folders deterministically |
| SO-F-S19-82 | D85 | merged | feat/super-ontology (App 车道 N 并入) | 8b92507aa → 8b92507aa | — | apps/super-ontology-desktop/ | Correct the T2 scenarios against what the app does |
| SO-F-S19-83 | D85 | merged | feat/super-ontology (App 车道 N 并入) | a584fce1d → a584fce1d | — | apps/super-ontology-desktop/ | Keep the fake engine for the T1 scenarios that declare it and stop the packaged query run  |
| SO-F-S19-84 | D85 | merged | feat/super-ontology (App 车道 N 并入) | e2260d60e → 644c48d81 | — | apps/super-ontology-desktop/ | Pick a dialog's own button, scroll to a control below the fold and run the boot check in b |
| SO-F-S19-90 | D88 | merged | feat/super-ontology (App 车道 P 并入) | 5170e7c94 → 5170e7c94 | — | apps/super-ontology-desktop/ | Record a real Builder draft of a synthetic shop as test data |
| SO-F-S19-91 | D88 | merged | feat/super-ontology (App 车道 P 并入) | 65a45c243 → 65a45c243 | — | apps/super-ontology-desktop/ | Add all seventeen database kinds and the build review to the app |
| SO-F-S19-92 | D88 | merged | feat/super-ontology (App 车道 P 并入) | f8c460bcd → 9708eccae | — | apps/super-ontology-desktop/ | Prove the database path from Add to Query in the app |
| SO-B-S16-15 | D85 | merged | feat/super-ontology (车道 R 并入) | f2a199878 → f2a199878 | — | — | Bundle every helper oo starts and check they match the build |
| SO-F-S19-93 | D85 | merged | feat/super-ontology (车道 R 并入) | a408bd04f → a408bd04f | — | — | Show a database-only atlas without a warning and draw links inside a group |
| SO-F-S19-94 | D85 | merged | feat/super-ontology (车道 R 并入) | cb2c77326 → cb2c77326 | — | — | Bundle every helper oo starts into the app and refuse a mismatched set |
| SO-F-S19-95 | D85 | merged | feat/super-ontology (车道 R 并入) | 808626dd0 → 728f8a07a | — | — | Let the live database scenario stop after the doctor |
| SO-F-S19-97 | D88 | merged | feat/super-ontology (App 车道 T 并入) | 49b437d06 → 49b437d06 | — | apps/super-ontology-desktop/ | Draw the atlas as precise growth: dots, squares, hull outlines, straight links |
| SO-F-S19-98 | D88 | merged | feat/super-ontology (App 车道 T 并入) | 084371f48 → 084371f48 | — | apps/super-ontology-desktop/ | Show the database tables and relationships in the atlas list and a version tick |
| SO-F-S19-99 | D88 | merged | feat/super-ontology (App 车道 T 并入) | b705c430d → 6d658be49 | — | apps/super-ontology-desktop/ | Summarise cross-group links, cap mark size, pixel-grid canvas; single-database column layout |
| SO-F-S20-01 | D46 | merged | feat/super-ontology (App 车道 W 并入) | 85a6b7ff1 → 85a6b7ff1 | — | apps/super-ontology-desktop/ | Stop T2 screen recordings when a scenario ends and cap them |
| SO-F-S20-02 | D46 | merged | feat/super-ontology (App 车道 W 并入) | f93da8d44 → f93da8d44 | — | apps/super-ontology-desktop/ | Expose the freshness state and the change-ring count on the page |
| SO-F-S20-03 | D46 | merged | feat/super-ontology (App 车道 W 并入) | 916224529 → 916224529 | — | apps/super-ontology-desktop/ | Add the P07 auto-sync scenario and its measuring harness |
| SO-F-S20-04 | D46 | merged | feat/super-ontology (App 车道 W 并入) | e7260e8c9 → e7260e8c9 | — | apps/super-ontology-desktop/ | Re-check a held mass delete on the engine's own syncs and keep a lone file from tripping it |
| SO-F-S20-05 | D46 | merged | feat/super-ontology (App 车道 W 并入) | dc9584d40 → dc9584d40 | — | apps/super-ontology-desktop/ | Do the P07 drop through the upload panel and summarise its timings |
| SO-F-S20-06 | D46 | merged | feat/super-ontology (App 车道 W 并入) | 47d47c287 → 47d47c287 | — | apps/super-ontology-desktop/ | Fix uploads from the source page: pass the bridge real File objects |
| SO-F-S20-07 | D46 | merged | feat/super-ontology (App 车道 W 并入) | 53743369a → 53743369a | — | apps/super-ontology-desktop/ | Send a full sync when a changed path has vanished from disk |
| SO-F-S20-08 | D46 | merged | feat/super-ontology (App 车道 W 并入) | 666ad86ac → 666ad86ac | — | apps/super-ontology-desktop/ | Make P07 repeatable and exact about renames, deletes and the atlas |
| SO-F-S20-09 | D46 | merged | feat/super-ontology (App 车道 W 并入) | 780bc36ec → 780bc36ec | — | apps/super-ontology-desktop/ | Judge P07 evidence per file, retry lost clicks and report steps that were not run |
| SO-F-S20-20 | D85 | merged | feat/super-ontology (App 车道 Z 并入) | 0b0be21ff → 0b0be21ff | — | apps/super-ontology-desktop/ | List agent tokens with state=all so a revoked token shows as revoked |
| SO-F-S20-10 | D86 | merged | feat/super-ontology (App 车道 Y 并入) | ff450e417 → ff450e417 | — | apps/super-ontology-desktop/ | Move native strings to JSON catalogs and fill their gaps |
| SO-F-S20-11 | D86 | merged | feat/super-ontology (App 车道 Y 并入) | ac9b79159 → ac9b79159 | — | apps/super-ontology-desktop/ | Strengthen check:i18n and close the gaps it finds |
| SO-F-S20-12 | D86 | merged | feat/super-ontology (App 车道 Y 并入) | 1e330026d → 1e330026d | — | apps/super-ontology-desktop/ | Record the vocabulary shared with the sibling desktop app and test it |
| SO-F-S20-13 | D86 | merged | feat/super-ontology (App 车道 Y 并入) | 4a31a3e73 → 4a31a3e73 | — | apps/super-ontology-desktop/ | Show engine and CLI errors in the language of the app, with the code on a details line |
| SO-F-S20-14 | D86 | merged | feat/super-ontology (App 车道 Y 并入) | 3285ce54e → 3285ce54e | — | apps/super-ontology-desktop/ | Bring the language setting up to the sibling app's behaviour |
| SO-F-S20-15 | D86 | merged | feat/super-ontology (App 车道 Y 并入) | 1e941c723 → 1e941c723 | — | apps/super-ontology-desktop/ | Keep the CLI cause first and settle terms after the visual review |
| SO-F-S20-16 | D86 | merged | feat/super-ontology (App 车道 Y 并入) | 912375045 → 912375045 | — | apps/super-ontology-desktop/ | Add a locale screenshot tool and a language switch for the T1 runs |
| SO-F-S20-30 | D46 | merged | feat/super-ontology (App 车道 W 并入) | 28193e5b1 → 28193e5b1 | — | apps/super-ontology-desktop/ | Drive the P07 open panel with checks and report the attempts it took |
| SO-F-S20-40 | D88 | merged | feat/super-ontology (App 车道 AA 并入) | a68cf24ef → a68cf24ef | — | apps/super-ontology-desktop/ | Add the Ask loop in the main process over a read-only agent token |
| SO-F-S20-41 | D88 | merged | feat/super-ontology (App 车道 AA 并入) | ee6a70f37 → ee6a70f37 | — | apps/super-ontology-desktop/ | Add the Ask tab under Ontology |
| SO-F-S20-42 | D88 | merged | feat/super-ontology (App 车道 AA 并入) | 261dabeb6 → 261dabeb6 | — | apps/super-ontology-desktop/ | Make what is not connected clear in the atlas, Sources and Home |
| SO-F-S20-43 | D88 | merged | feat/super-ontology (App 车道 AA 并入) | 4993bdebe → 4993bdebe | — | apps/super-ontology-desktop/ | Read a model address kept in a file reference |
| SO-F-S20-44 | D88 | merged | feat/super-ontology (App 车道 AA 并入) | bc5993740 → bc5993740 | — | apps/super-ontology-desktop/ | Let a T1 run use the compose engine and a 2x window |
| SO-F-S20-45 | D88 | merged | feat/super-ontology (App 车道 AA 并入) | 8c16d80da → 8c16d80da | — | apps/super-ontology-desktop/ | Fix the Ask request check, number cited sources, and read files honestly |
| SO-F-S20-46 | D88 | merged | feat/super-ontology (App 车道 AA 并入) | 62ed3bf4d → 62ed3bf4d | — | apps/super-ontology-desktop/ | Lock the Ask and Not connected screens with visual baselines |
| SO-F-S20-47 | D88 | merged | feat/super-ontology (App 车道 AA 并入) | 25f90789e → 25f90789e | — | apps/super-ontology-desktop/ | Check for disagreements before the Ask answer states an owner, date, number or status |
| SO-F-S20-48 | D88 | merged | feat/super-ontology (App 车道 AA 并入) | bcb87c7de → bcb87c7de | — | apps/super-ontology-desktop/ | Refresh the Ask baselines for human anchors, the clear icon and kept-together markers |
| SO-F-S20-50 | D74 | merged | feat/super-ontology (车道 AC 并入) | 1038e4120 → 1038e4120 | — | — | Judge Not connected across files, not inside one |
| SO-F-S20-51 | D74 | merged | feat/super-ontology (车道 AC 并入) | af9b11a6a → af9b11a6a | — | — | Extract recordings without waiting for speaker names |
| SO-F-S20-52 | D74 | merged | feat/super-ontology (车道 AC 并入) | 2093bc5e0 → 2093bc5e0 | — | — | Ask for speaker names in Review |
| SO-F-S20-53 | D74 | merged | feat/super-ontology (车道 AC 并入) | 33991d4b8 → 33991d4b8 | — | — | Lock the engine image built with provisional speakers |
| SO-F-S20-54 | D74 | merged | feat/super-ontology (车道 AC 并入) | ac725a2e3 → ac725a2e3 | — | — | Let the acceptance driver take its Docker lane from the environment |
| SO-F-S20-55 | D74 | merged | feat/super-ontology (车道 AC 并入) | f2d3db620 → f2d3db620 | — | — | Keep no_relations as the id of the cross-file reason |
| SO-F-S20-56 | D74 | merged | feat/super-ontology (车道 AC 并入) | d590c243a → d590c243a | — | — | Hold the statements of an unconfirmed speaker again |
| SO-F-S20-57 | D74 | merged | feat/super-ontology (车道 AC 并入) | 28627c712 → 28627c712 | — | — | Assert the speaker hold in the live and unit tests |
| SO-F-S20-58 | D74 | merged | feat/super-ontology (车道 AC 并入) | 3d1d9e6a7 → 3d1d9e6a7 | — | — | Show a recording that waits for speaker confirmation as such |
| SO-F-S20-60 | D85 | merged | feat/super-ontology (App 车道 AE 并入) | fbd90f0d6 → fbd90f0d6 | — | — | Lock the engine image rebuilt for the final build |
| SO-F-S20-61 | D85 | merged | feat/super-ontology (App 车道 AE 并入) | 92e3eac46 → 92e3eac46 | — | — | Let the model dialog take file paths for the address and the key |
| SO-F-S20-62 | D85 | merged | feat/super-ontology (App 车道 AE 并入) | 08766f368 → 08766f368 | — | — | Press Show more before reading the Not connected list |
| SO-F-S20-63 | D85 | merged | feat/super-ontology (App 车道 AE 并入) | a194299cb → a194299cb | — | — | Allow the model on folders the app creates on this computer |
| SO-F-S20-64 | D85 | merged | feat/super-ontology (车道 AE2 并入) | 6339adc55 → 6339adc55 | — | — | Lock the engine image with the settle step |
| SO-F-S20-65 | D85 | merged | feat/super-ontology (App 车道 AE4 并入) | a87935859 → a87935859 | — | deploy/docs/images.lock.json | Lock the engine image built for the redelivered app |
| SO-F-S20-72 | D83 | merged | feat/super-ontology (车道 AF 并入) | e49a7c60a → e49a7c60a | — | — | Keep the engine logs of a failed T2 attempt and name the missing engine |
| SO-F-S20-71 | D83 | merged | feat/super-ontology (车道 AF 并入) | e3fa74d35 → e3fa74d35 | — | — | Show a failed engine start on the first-run screen with Try again and the log path |
| SO-F-S20-66 | D85 | merged | feat/super-ontology (车道 AE5 并入) | cc0a236bb → cc0a236bb | — | — | Lock the engine image with migration 0154 and the drive-scale fixes |
| SO-F-S21-01 | D86 | merged | feat/super-ontology (App 车道 AG 并入) | 90eddf666 → 90eddf666 | — | apps/super-ontology-desktop/ | Walk dropped folders with the engine's ignore rules and say what was skipped |
| SO-F-S21-02 | D86 | merged | feat/super-ontology (App 车道 AG 并入) | ff2765999 → ff2765999 | — | apps/super-ontology-desktop/ | Turn a dropped folder into a one-use source grant and describe the richer upload answer |
| SO-F-S21-03 | D86 | merged | feat/super-ontology (App 车道 AG 并入) | a54d28ea1 → a54d28ea1 | — | apps/super-ontology-desktop/ | Queue drops of any size and keep early progress pushes |
| SO-F-S21-04 | D86 | merged | feat/super-ontology (App 车道 AG 并入) | 1f4bfce34 → 0758df153 | — | apps/super-ontology-desktop/ | Take back the copy when a dropped folder is kept in sync instead |
| SO-F-S21-05 | D86 | merged | feat/super-ontology (App 车道 AG 并入) | 28c84e837 → ce8bbc279 | — | apps/super-ontology-desktop/ | Read upload progress per item in the existing upload service tests |
| SO-F-S21-06 | D86 | merged | feat/super-ontology (App 车道 AG 并入) | 28d76638b → 28d76638b | — | apps/super-ontology-desktop/ | Test the overlay states, the drop and the toast as components |
| SO-F-S21-07 | D86 | merged | feat/super-ontology (App 车道 AG 并入) | 6bca6fec6 → 6bca6fec6 | — | apps/super-ontology-desktop/ | Test dragging files and folders into the app against a real engine |
| SO-F-S21-08 | D86 | merged | feat/super-ontology (App 车道 AG 并入) | 431a85208 → d56adcd5c | — | apps/super-ontology-desktop/ | Give the upload list rows room for a folder line |
| SO-F-S21-09 | D86 | merged | feat/super-ontology (App 车道 AG 并入，协调者台账) | 83a155957 → 83a155957 | — | — | Add the ledger rows for lane AG |
| SO-F-S21-10 | D83 | merged | feat/super-ontology (车道 AH 并入) | c43a02729 → c43a02729 | — | — | Share one local engine between the app and the oo CLI |
| SO-F-S21-11 | D83 | merged | feat/super-ontology (车道 AH 并入) | 89844c292 → 89844c292 | — | — | Show database and file changes made by the CLI or an agent as they happen |
| SO-F-S21-12 | D83 | merged | feat/super-ontology (车道 AH 并入) | 53c34e101 → cabe7211d | — | — | Keep the actor tag's test attributes on an element the design system keeps |
| SO-F-S21-13 | D83 | merged | feat/super-ontology (车道 AH 并入) | 5eefc480a → 09a7de8ca | — | — | Expose what the Atlas knows now, and finish the live CLI run |
| SO-F-S21-19 | D83 | merged | feat/super-ontology (车道 AH 并入，协调者台账) | 0170cf98b → 790ff357c | — | — | Ledger rows for lanes AH and AG |
| SO-F-S21-30 | D83 | merged | feat/super-ontology (车道 AH2 并入) | fc8ba9fe0 → fc8ba9fe0 | — | — | Show one "My files" folder for the CLI and the app |
| SO-F-S21-31 | D83 | merged | feat/super-ontology (车道 AH2 并入) | 68427238e → e2d6fbc9e | — | — | Keep one drive refresh per engine start and update the drop toast tests |
| SO-F-S21-40 | D86 | merged | feat/super-ontology (车道 AI 并入) | 9882a119e → 9882a119e | — | — | Read .gitignore, .ooignore and code project defaults the way the engine does |
| SO-F-S21-41 | D86 | merged | feat/super-ontology (车道 AI 并入) | 5b833e7bb → 063b60ebb | — | — | Keep the default suite's file churn small: the 25,000 file run belongs to T1 |
| SO-F-S21-42 | D86 | merged | feat/super-ontology (车道 AI 并入) | eda990681 → ddb918464 | — | — | Let the long link action wrap in the toast, and add the visual baselines of the new states |
| SO-F-S21-43 | D86 | merged | feat/super-ontology (车道 AI 并入) | d4b5ba5a4 → 42e4ec90a | — | — | Keep reading Not connected from starting over while a big folder publishes |
| SO-F-S21-44 | D86 | merged | feat/super-ontology (车道 AI 并入) | 6d430c437 → 6d430c437 | — | — | Watch a linked code folder by the same rules as the engine and the copy |
| SO-F-S21-45 | D86 | merged | feat/super-ontology (车道 AI 并入) | 47d257a8b → 47d257a8b | — | — | Add the off-screen T1 runs for a code project drop and its timings |
| SO-F-S21-49 | D86 | merged | feat/super-ontology (车道 AI 并入，协调者台账) | b74249d5c → b74249d5c | — | — | Ledger rows for lane AI |
| SO-F-S22-40 | D98 | merged | feat/super-ontology (车道 AL 并入) | 6d77e4775 → 6d77e4775 | — | — | Serve recordings with their media type so the native player can start anywhere |
| SO-F-S22-41 | D98 | merged | feat/super-ontology (车道 AL 并入) | 6a0944595 → 6a0944595 | — | — | Add a media preview with a synced transcript and a recording class badge |
| SO-F-S22-42 | D98 | merged | feat/super-ontology (车道 AL 并入) | 91e643ac4 → 91e643ac4 | — | — | Name the speakers of a recording in Review beside a player, without blocking anything |
| SO-F-S22-43 | D98 | merged | feat/super-ontology (车道 AL 并入) | 235731670 → 235731670 | — | — | Play a recording from the file panel and from an evidence mark at the right second |
| SO-F-S22-44 | D98 | merged | feat/super-ontology (车道 AL 并入) | 5f9623cdb → 5f9623cdb | — | — | Let the page's own policy admit recordings served by the capability protocol |
| SO-F-S22-45 | D98 | merged | feat/super-ontology (车道 AL 并入) | 23f4c9e26 → 23f4c9e26 | — | — | Cover the speaker flow in visual baselines and an off-screen end-to-end run |
| SO-F-S22-46 | D98 | merged | feat/super-ontology (车道 AL 并入) | 02ead02bb → 02ead02bb | — | — | Show unnamed speakers as the speaker of their meeting and list the ones to name on the file |
| SO-F-S22-20 | D94 | merged | feat/super-ontology (车道 AK 并入) | 5e4873988 → 5e4873988 | — | — | Add the document profile to file summaries and a label for it |
| SO-F-S22-21 | D94 | merged | feat/super-ontology (车道 AK 并入) | f0ea23887 → f0ea23887 | — | — | Show the file profile and treat indexed-only files as fine |
| SO-F-S22-23 | D94 | merged | feat/super-ontology (车道 AK 并入) | 20f875950 → 20f875950 | — | — | Add file class and reason to the profile model and rename paper to reference |
| SO-F-S22-24 | D94 | merged | feat/super-ontology (车道 AK 并入) | 248f49751 → 248f49751 | — | — | Keep files outside the ontology out of Not connected |
| SO-F-S22-25 | D94 | merged | feat/super-ontology (车道 AK 并入) | 72e91705a → 72e91705a | — | — | Let people set the class of a file or a folder |
| SO-F-S22-26 | D94 | merged | feat/super-ontology (车道 AK 并入) | 777c59c3e → 777c59c3e | — | — | Update the file panel snapshots for the class line and the class select |
| SO-F-S22-80 | D85 | merged | feat/super-ontology (车道 AN 并入) | 40d476e36 → 40d476e36 | — | — | Test the drive watcher by wake-ups and scheduled delays, not CPU share or elapsed time |
| SO-F-S22-81 | D85 | merged | feat/super-ontology (车道 AN 并入) | 9d54f9b53 → 9d54f9b53 | — | — | Arm the real watcher tests with an in-order barrier before they change anything |
| SO-F-S22-60 | D93 | merged | feat/super-ontology (车道 AM 并入) | 906a3d624 → 906a3d624 | — | — | Bridge the structure layer to the app: read, induce, decide and the ontology YAML |
| SO-F-S22-61 | D93 | merged | feat/super-ontology (车道 AM 并入) | 92a0c0238 → 92a0c0238 | — | — | Show the structure layer in the Ontology view: a Structure tab, the class panel on the atlas, the YAML in Files |
| SO-F-S22-62 | D93 | merged | feat/super-ontology (车道 AM 并入) | c7b186d7a → c7b186d7a | — | — | Test the Structure tab in Chinese and German |
| SO-F-S22-63 | D93 | merged | feat/super-ontology (车道 AM 并入) | ed03697cb → ed03697cb | — | — | Show the version type of a structure property, and score completeness on the held-out corpus |
| SO-F-S23-01 | D85 | merged | feat/super-ontology (车道 AE6 并入) | 45591ed63 → 45591ed63 | — | — | Update the FM5 replay of not-certain supersessions to D97 |
| SO-F-S23-02 | D85 | merged | feat/super-ontology (车道 AE6 并入) | 1612f7ade → 1612f7ade | — | — | Expect the type_precision key in the fixed docs report schema |
| SO-F-S23-03 | D85 | merged | feat/super-ontology (车道 AE6 并入) | cbd655dca → cbd655dca | — | — | Count the structure_alignment table in the install tests |
| SO-F-S23-04 | D85 | merged | feat/super-ontology (车道 AE6 并入) | 32a36006a → 32a36006a | — | — | Pin sherpa-onnx and the Whisper base weights for the media-parser image |
| SO-F-S23-05 | D85 | merged | feat/super-ontology (车道 AE6 并入) | 4a7cfd331 → 4a7cfd331 | — | — | Lock the engine and media-parser images of the convergence build |
| SO-F-S23-06 | D85 | merged | feat/super-ontology (车道 AE6 并入) | 05ae2a6ae → 05ae2a6ae | — | — | Do not let a pending speaker item hold a file that has nothing left to publish |
| SO-F-S23-07 | D85 | merged | feat/super-ontology (车道 AE6 并入) | 4a0e73727 → 4a0e73727 | — | — | Follow the shared engine and the streaming upload in the packaged-app acceptance run |
| SO-F-S23-08 | D85 | merged | feat/super-ontology (车道 AE6 并入) | b47498d95 → b47498d95 | — | — | Lock the engine image with the speaker-aware settle step |
| SO-F-S23-09 | D85 | merged | feat/super-ontology (车道 AE6 并入) | e984f0845 → e984f0845 | — | — | Record the S22 convergence evidence |
| SO-F-S23-10 | D85 | merged | feat/super-ontology (车道 AE7 并入) | a3361a2dc → a3361a2dc | — | — | Wait for a dying oo serve instead of reporting another instance starting |
| SO-F-S23-11 | D85 | merged | feat/super-ontology (车道 AE7 并入) | fdc3adbdb → fdc3adbdb | — | — | Make the 10k-file watcher idle test event-driven |
| SO-F-S23-12 | D85 | merged | feat/super-ontology (车道 AE7 并入) | c94d0c6c6 → c94d0c6c6 | — | — | Map the D94 and D98 app states onto the unlinked gold's reasons |
| SO-F-S23-13 | D85 | merged | feat/super-ontology (车道 AE7 并入) | de4d296c3 → de4d296c3 | — | — | Regenerate the media-parser SBOM for the Whisper and sherpa-onnx image and pass the licence gate |
| SO-F-S23-14 | D85 | merged | feat/super-ontology (车道 AE7 并入) | 1429ab061 → 1429ab061 | — | — | Pin the media-parser image with the regenerated SBOM |
| SO-F-S23-15 | D85 | merged | feat/super-ontology (车道 AE7 并入) | e11473b4f → e11473b4f | — | — | Add the AE7 follow-up results to the convergence evidence |
| SO-F-S23-16 | D83 | merged | feat/super-ontology | 8e1bbe141 → 8e1bbe141 | — | tools/doc-parser/runtime-artifacts.json | Point the OpenCV runtime pin at the published release asset |
| SO-F-S23-20 | D83 | merged | feat/super-ontology | fb19141ee → fb19141ee | — | docs/reference/cli.md, docs/guide/troubleshooting.md | Regenerate the CLI and error references against the default build |
| SO-F-S23-21 | D83 | merged | feat/super-ontology | 13ac96224 → 13ac96224 | — | Cargo.lock, crates/xtask/src/supply.rs, supply-chain/url-security-fix-seven-day.json | Update url to 2.5.9 for RUSTSEC-2026-0592 |
| SO-F-S23-22 | D83 | merged | feat/super-ontology | e12cc1495 → e12cc1495 | — | supply-chain/secret-review.json | Review the secret scanner findings and re-bind stale reviews |
| SO-F-S23-23 | D83 | merged | feat/super-ontology | 25cc2ee71 → 25cc2ee71 | — | benchmarks/ab/fixtures/replay-report.linux-amd64.json | Record the A/B replay report for the current benchmark binaries |
| SO-F-S23-24 | D81 | merged | feat/super-ontology | 6eff3c3a3 → 6eff3c3a3 | — | .github/workflows/formal.yml | Stop two formal CI failures that were not model violations |
| SO-F-S23-30 | D83 | merged | feat/super-ontology | e65dcbee5 → e65dcbee5 | — | tools/media-parser/Dockerfile, tools/media-parser/campplus-export.lock, tools/ocr-parser/Dockerfile | Build the media-parser image from the repository alone |
| SO-F-S23-31 | D83 | merged | feat/super-ontology | 5d9f4c203 → 5d9f4c203 | — | .github/workflows/docs-images.yml, deploy/docs/docker-bake.hcl, deploy/docs/release_summary.py | Add the docs-images workflow for CI-only publication |
| SO-F-S23-32 | D83 | merged | feat/super-ontology | 47298a581 → 47298a581 | — | docs/engineering/release.md, docs/engineering/ci.md, specs/super-ontology/10-backend.md, deploy/docs/images.lock.json | Document CI-only publication of the docs runtime images after merge |
| SO-F-S23-33 | D83 | merged | feat/super-ontology | (this commit) | — | execution/super-ontology/ledger.md | Add the ledger rows for the docs-images workflow |
| SO-F-S23-34 | D83 | merged | feat/super-ontology | (this commit) | — | crates/xtask/src/supply.rs, supply-chain/url-security-fix-seven-day.json | Remove the temporary age-floor waiver for url 2.5.9 and form_urlencoded 1.2.3 (owner decision); gate-dependencies waits for the 7-day floor, 2026-11-27 |
| SO-F-S23-35 | D83 | merged | feat/super-ontology | (this commit) | — | supply-chain/secret-review.json | Re-bind the xtask supply review after the waiver removal |
| SO-B-S13-01 | L8 | merged | feat/super-ontology (车道 C 分支并入) | 5c3b92efd → 1745581d1 | — | testing/super-ontology/evidence/L8/ | 可选 pgvector 迁移 0144；服务端带 pgvector 时安装向量表 |
| SO-B-S13-02 | L8 | merged | feat/super-ontology (车道 C 分支并入) | 00ea0bc8e → 00ea0bc8e | — | testing/super-ontology/evidence/L8/ | 可选本地向量：BRIEF 其他相关与 resolve 召回 |
| SO-B-S13-03 | L8 | merged | feat/super-ontology (车道 C 分支并入) | ddf16f740 → 7d00afb0f | — | testing/super-ontology/evidence/L8/ | 1 万文件规模基准 |
| SO-B-S14-01 | L8 | merged | feat/super-ontology (车道 C 分支并入) | e4169b128 → e4169b128 | — | testing/super-ontology/evidence/L8/ | 无 Docker 运行时评审与原型测量（ADR-S14） |
| SO-B-S21-10 | D83 | merged | feat/super-ontology (车道 AH 并入) | 771be6ca6 → 771be6ca6 | — | — | Attach the actor of a request to the events it causes |
| SO-B-S21-11 | D83 | merged | feat/super-ontology (车道 AH 并入) | 7ec03529b → 7ec03529b | — | — | Add POST /v1/docs/events:publish for database pipeline events |
| SO-B-S21-12 | D83 | merged | feat/super-ontology (车道 AH 并入) | 85321932d → 85321932d | — | — | Announce database pipeline changes from the CLI to a running serve |
| SO-B-S21-13 | D83 | merged | feat/super-ontology (车道 AH 并入) | a48ee5674 → a48ee5674 | — | — | Record docs-api 1.13 in the contract changelog |
| SO-B-S21-14 | D83 | merged | feat/super-ontology (车道 AH 并入) | 5acf49df7 → 5acf49df7 | — | — | Let an agent name itself to the engine with OO_AGENT_NAME |
| SO-B-S21-15 | D83 | merged | feat/super-ontology (车道 AH 并入) | f4526f574 → f4526f574 | — | — | Announce review items that enter or leave a queue without a decision |
| SO-B-S21-30 | D83 | merged | feat/super-ontology (车道 AH2 并入) | 278ad7569 → 278ad7569 | — | — | Make the CLI's managed folder the app's "My files" |
| SO-B-S21-40 | D86 | merged | feat/super-ontology (车道 AI 并入) | df89d8d7c → 0d69d7cd8 | — | — | Cover the default skip table in the shared ignore fixture |
| SO-B-S21-41 | D86 | merged | feat/super-ontology (车道 AI 并入) | 8dfd5bd8c → 8dfd5bd8c | — | — | Skip code project folders and honour .gitignore and .ooignore in fs Drives |
| SO-B-S21-42 | D86 | merged | feat/super-ontology (车道 AI 并入) | c235c3cad → c235c3cad | — | — | Hash a file after loading its own row, not every row of its Drive |
| SO-B-S21-43 | D86 | merged | feat/super-ontology (车道 AI 并入) | 450b64c91 → 450b64c91 | — | — | Anchor merge proposals on the published keys, one branch for each end |
| SO-B-S21-44 | D86 | merged | feat/super-ontology (车道 AI 并入) | d95750b90 → d95750b90 | — | — | Lift the per-Drive file cap and index jobs by file |
| SO-B-S22-40 | D98 | merged | feat/super-ontology (车道 AL 并入) | 9f9c57139 → 9f9c57139 | — | — | Classify a recording as a conversation or not before its speakers are told apart |
| SO-B-S22-41 | D98 | merged | feat/super-ontology (车道 AL 并入) | 2bab76d38 → 2bab76d38 | — | — | Publish a conversation under placeholder speakers and re-attribute when they are named |
| SO-B-S22-42 | D98 | merged | feat/super-ontology (车道 AL 并入) | 2e47ed99c → 2e47ed99c | — | — | Report the class of a recording on the file detail and record docs-api 1.14 |
| SO-B-S22-43 | D98 | merged | feat/super-ontology (车道 AL 并入) | 81d8c1f65 → 81d8c1f65 | — | — | Ask the model about a recording whose voices were merged and ignore stray speaker labels |
| SO-B-S22-44 | D98 | merged | feat/super-ontology (车道 AL 并入) | 0772dbff1 → 0772dbff1 | — | — | Test the recording class on real pipeline output of synthetic speech and music |
| SO-B-S22-45 | D98 | merged | feat/super-ontology (车道 AL 并入) | 28c7d10d8 → 28c7d10d8 | — | — | Measure the acceptance audio by class, speaker items and naming through the engine |
| SO-B-S22-47 | D98 | merged | feat/super-ontology (车道 AL 并入) | 1a5b89089 → 1a5b89089 | — | — | Type a recording, its meeting and its participants, and read nothing from a media asset |
| SO-B-S22-48 | D98 | merged | feat/super-ontology (车道 AL 并入) | 638f64d25 → 638f64d25 | — | — | Name a speaker's placeholder after its meeting and move the meeting's participant to the named Person |
| SO-B-S22-49 | D98 | merged | feat/super-ontology (车道 AL 并入) | 029361168 → 029361168 | — | — | Record the Recording and Meeting types and the media classes in docs-api 1.14 and spec 17.4 |
| SO-B-S22-46 | D98 | merged | feat/super-ontology (车道 AL 并入) | 1078c0af9 → 1078c0af9 | — | — | WIP: paused by owner (media classes and speakers) |
| SO-B-S22-50 | D98 | merged | feat/super-ontology (车道 AL 并入) | 0d8e761f5 → 0d8e761f5 | — | — | Keep speaker naming out of the review counts and re-validate the class on eight synthetic meetings |
| SO-B-S22-51 | D98 | merged | feat/super-ontology (车道 AL 并入) | 84867d396 → 84867d396 | — | — | Count speakers by the eigengap of centred embeddings and skip transcription of music and assets |
| SO-B-S22-52 | D98 | merged | feat/super-ontology (车道 AL 并入) | c717e44e3 → c717e44e3 | — | — | Tell the sidecar a file name says media asset and refresh the held-out meetings with the new clustering |
| SO-B-S22-53 | D98 | merged | feat/super-ontology (车道 AL 并入) | dc3d6f85e → dc3d6f85e | — | — | Run the class decision against the real model on the recorded transcripts |
| SO-B-S22-54 | D98 | merged | feat/super-ontology (车道 AL 并入) | f89ceaad6 → f89ceaad6 | — | — | Route each audio chunk to the right recognizer by its spoken language |
| SO-B-S22-55 | D98 | merged | feat/super-ontology (车道 AL 并入) | ee197f881 → ee197f881 | — | — | Re-record the acceptance calls and the held-out meetings with the language-routed transcripts |
| SO-B-S22-20 | D94 | merged | feat/super-ontology (车道 AK 并入) | 391f5904e → 391f5904e | — | — | Add the tree-sitter parsers for the code profile |
| SO-B-S22-21 | D94 | merged | feat/super-ontology (车道 AK 并入) | ee5379486 → ee5379486 | — | — | Read documents by profile: instruction docs, code and papers |
| SO-B-S22-22 | D94 | merged | feat/super-ontology (车道 AK 并入) | d7a9681db → d7a9681db | — | — | Store the profile on the file and serve it in the file list |
| SO-B-S22-23 | D94 | merged | feat/super-ontology (车道 AK 并入) | 2ab645fc1 → 2ab645fc1 | — | — | Keep the Decisions folder of a business Drive out of the instruction profile |
| SO-B-S22-24 | D94 | merged | feat/super-ontology (车道 AK 并入) | 4ee13d4d1 → 4ee13d4d1 | — | — | Add the profile test set: a synthetic repo, three papers and the frozen gold |
| SO-B-S22-25 | D94 | merged | feat/super-ontology (车道 AK 并入) | 98a721cb5 → 98a721cb5 | — | — | Read style guides, bare file names and prose commands; list trait methods |
| SO-B-S22-26 | D94 | merged | feat/super-ontology (车道 AK 并入) | 259a759d9 → 259a759d9 | — | — | Add the offline dump and the scorer of the profile measurement |
| SO-B-S22-27 | D94 | merged | feat/super-ontology (车道 AK 并入) | 987cd4334 → 987cd4334 | — | — | Read a paper as the PDF parser breaks it into blocks |
| SO-B-S22-28 | D94 | merged | feat/super-ontology (车道 AK 并入) | 14631c4dd → 14631c4dd | — | — | Write the test papers with gaps the parser's layout step can split |
| SO-B-S22-29 | D94 | merged | feat/super-ontology (车道 AK 并入) | 7e4cfc894 → 7e4cfc894 | — | — | Add the live run of the profile set and the headless Ask on it |
| SO-B-S22-30 | D94 | merged | feat/super-ontology (车道 AK 并入) | c758f54bb → c758f54bb | — | — | Let a paper wait for review with the model on and check promotions by label |
| SO-B-S22-31 | D94 | merged | feat/super-ontology (车道 AK 并入) | ecb55ab9e → ecb55ab9e | — | — | Put the 0153 upgrade test back to the state 0153 leaves |
| SO-B-S22-32 | D94 | merged | feat/super-ontology (车道 AK 并入) | 2fec7481b → 2fec7481b | — | — | WIP: paused by owner |
| SO-B-S22-33 | D94 | merged | feat/super-ontology (车道 AK 并入) | 5c976b490 → 5c976b490 | — | — | Admit files by class: instruction, code and reference read for structure only |
| SO-B-S22-34 | D94 | merged | feat/super-ontology (车道 AK 并入) | 88f6b962d → 88f6b962d | — | — | Serve the class of a file and let a person overrule it |
| SO-B-S22-35 | D94 | merged | feat/super-ontology (车道 AK 并入) | ae720f6ce → ae720f6ce | — | — | Put the instruction files in the BRIEF as working conventions |
| SO-B-S22-36 | D94 | merged | feat/super-ontology (车道 AK 并入) | c6fced5f7 → c6fced5f7 | — | — | Add oo docs profile to set the class of a file or a directory |
| SO-B-S22-37 | D94 | merged | feat/super-ontology (车道 AK 并入) | 777048100 → 777048100 | — | — | Replace the profile test set by a mixed corpus with frozen class gold |
| SO-B-S22-38 | D94 | merged | feat/super-ontology (车道 AK 并入) | ea718960e → ea718960e | — | — | Score admission: class accuracy, zero entities from non-records, BRIEF |
| SO-B-S22-39 | D94 | merged | feat/super-ontology (车道 AK 并入) | eeac9e600 → eeac9e600 | — | — | Describe the admission corpus and let a model made record settle indexed |
| SO-B-S22-59 | D98 | merged | feat/super-ontology (车道 AL 并入，协调者台账) | 891006a0a → 891006a0a | — | — | Ledger rows for lane AL |
| SO-B-S22-80 | D85 | merged | feat/super-ontology (车道 AN 并入) | 5a06e8a8c → 5a06e8a8c | — | — | Fix the early publish query: a continued SQL line ended in two backslashes |
| SO-B-S22-81 | D85 | merged | feat/super-ontology (车道 AN 并入) | fc3cba870 → fc3cba870 | — | — | Pin the extraction prompt version to its system text and re-pin the recorded keys |
| SO-B-S22-82 | D85 | merged | feat/super-ontology (车道 AN 并入) | 2678ff58e → 2678ff58e | — | — | Name the commit timestamp precondition of the event order test |
| SO-B-S22-83 | D85 | merged | feat/super-ontology (车道 AN 并入) | 431e614d6 → 431e614d6 | — | — | Give each document the share conformance test adds a date of its own |
| SO-B-S22-84 | D85 | merged | feat/super-ontology (车道 AN 并入) | 171f839f2 → 171f839f2 | — | — | Teach the live suite runner the profiles run and the model-only target |
| SO-B-S22-85 | D85 | merged | feat/super-ontology (车道 AN 并入) | f5563b49a → f5563b49a | — | — | Do not hold a partial file back for an alias candidate that nothing will decide |
| SO-B-S22-86 | D85 | merged | feat/super-ontology (车道 AN 并入) | ba8b2d0c3 → ba8b2d0c3 | — | — | Track the admission corpus log that the root ignore rule dropped |
| SO-B-S22-89 | D85 | merged | feat/super-ontology (车道 AN 并入，协调者台账) | 6c46e2bc5 → 6c46e2bc5 | — | — | Ledger rows for lane AN |
| SO-B-S22-60 | D93 | merged | feat/super-ontology (车道 AM 并入) | 143442470 → 1f5061cda | — | — | Add eight frozen questions to the structure corpus, for the secondary Ask measure |
| SO-B-S22-61 | D93 | merged | feat/super-ontology (车道 AM 并入) | 0f4e2b5cf → 0f4e2b5cf | — | — | Induce the structure layer of the record files like the columns of a table |
| SO-B-S22-62 | D93 | merged | feat/super-ontology (车道 AM 并入) | bde8d5037 → bde8d5037 | — | — | Read a record file of a structured class against its template, also with the model |
| SO-B-S22-63 | D93 | merged | feat/super-ontology (车道 AM 并入) | b6e74ee1f → b6e74ee1f | — | — | Serve the structure layer: classes, decisions and the ontology YAML of both sides |
| SO-B-S22-64 | D93 | merged | feat/super-ontology (车道 AM 并入) | 4cbc54f03 → 4cbc54f03 | — | — | Let a decision about the structure pass the reprocess gate and propose recurring fields |
| SO-B-S22-65 | D93 | merged | feat/super-ontology (车道 AM 并入) | dfec1280c → dfec1280c | — | — | Measure the structure layer through the real engine, before and after a person confirms it |
| SO-B-S22-66 | D93 | merged | feat/super-ontology (车道 AM 并入) | fa3ef3963 → fa3ef3963 | — | — | Let the L4 evaluation confirm the induced structure, and add the ontology-quality gate |
| SO-B-S22-67 | D93 | merged | feat/super-ontology (车道 AM 并入) | 737520820 → 737520820 | — | — | Link document properties to database tables by their values, and check the merged ontology |
| SO-B-S22-68 | D93 | merged | feat/super-ontology (车道 AM 并入) | f2999a873 → f2999a873 | — | — | Add oo docs structure: list, induce, decide and print the ontology YAML |
| SO-B-S22-69 | D93 | merged | feat/super-ontology (车道 AM 并入) | f5afb3655 → f5afb3655 | — | — | Follow migration 0160 in the install tests |
| SO-B-S22-70 | D93 | merged | feat/super-ontology (车道 AM 并入) | 89d3f8962 → 89d3f8962 | — | — | Add a held-out structure corpus and freeze its gold |
| SO-B-S22-71 | D93 | merged | feat/super-ontology (车道 AM 并入) | 5cc9b4875 → 5cc9b4875 | — | — | Score the held-out corpus: members per class, labels, types and relations |
| SO-B-S22-72 | D93 | merged | feat/super-ontology (车道 AM 并入) | 607c5f0eb → 607c5f0eb | — | — | Align field labels across languages into one class, and read level codes and versions |
| SO-B-S22-79 | D93 | merged | feat/super-ontology (车道 AM 并入，协调者台账) | ddef647bd → ddef647bd | — | — | Ledger rows for lane AM |
| SO-B-S22-01 | D96 | merged | feat/super-ontology (车道 AJ 并入) | 6f9cf0a9a → 6f9cf0a9a | — | — | Add the engineering corpus and a quality runner with entity and review-queue diagnostics |
| SO-B-S22-02 | D96 | merged | feat/super-ontology (车道 AJ 并入) | 60fa72ec1 → 57c175804 | — | — | State in the gate that what fails is dropped, not kept as text |
| SO-B-S22-03 | D96 | merged | feat/super-ontology (车道 AJ 并入) | 8148de176 → 8148de176 | — | — | Settle entity merges a program can settle instead of asking |
| SO-B-S22-04 | D96 | merged | feat/super-ontology (车道 AJ 并入) | bcbb79f01 → bcbb79f01 | — | — | Ask about low-confidence facts only when they contradict each other |
| SO-B-S22-05 | D96 | merged | feat/super-ontology (车道 AJ 并入) | eb131ba8e → eb131ba8e | — | — | Add the review-queue scale test and an engine hold for the acceptance scorers |
| SO-B-S22-06 | D96 | merged | feat/super-ontology (车道 AJ 并入) | 88c33d871 → b5445f293 | — | — | Format the guard test and name the evidence row type |
| SO-B-S22-07 | D96 | merged | feat/super-ontology (车道 AJ 并入) | e17ea772d → e17ea772d | — | — | Make the review and the test fixtures follow D91 |
| SO-B-S22-08 | D96 | merged | feat/super-ontology (车道 AJ 并入) | aae3224d2 → aae3224d2 | — | — | WIP: paused by owner |
| SO-B-S22-09 | D96 | merged | feat/super-ontology (车道 AJ 并入) | 726d56896 → 726d56896 | — | — | Make the gate and the queue follow D96 and D97 |
| SO-B-S22-10 | D96 | merged | feat/super-ontology (车道 AJ 并入) | 72476e8ac → 72476e8ac | — | — | Gate Thing entities like Terms |
| SO-B-S22-11 | D96 | merged | feat/super-ontology (车道 AJ 并入) | ef2869b0a → ef2869b0a | — | — | Fix the scale test paths so no two files collide |
| SO-B-S22-12 | D96 | merged | feat/super-ontology (车道 AJ 并入) | 8fc8644b6 → 8fc8644b6 | — | — | Name the acceptance runner's file listing route and drop a unit binding |
| SO-B-S22-13 | D96 | merged | feat/super-ontology (车道 AJ 并入) | 50e5ed8f7 → 50e5ed8f7 | — | — | Report type precision and count a bare mention as no fact |
| SO-B-S22-14 | D96 | merged | feat/super-ontology (车道 AJ 并入) | 3c71df731 → 3c71df731 | — | — | Mix instruction files, design notes with bare-word lists and moves into the scale folder |
| SO-B-S22-15 | D96 | merged | feat/super-ontology (车道 AJ 并入) | f10789131 → f10789131 | — | — | Let the acceptance runner also ask the questions through the CLI agent |
| SO-B-S22-17 | D96 | merged | feat/super-ontology (车道 AJ 并入) | 436526720 → 436526720 | — | — | Run the quality gate on the model's output as the model's output |
| SO-B-S22-18 | D96 | merged | feat/super-ontology (车道 AJ 并入) | e7d74e00a → e7d74e00a | — | — | Leave proper names to the cross-file count |
| SO-B-S22-19 | D96 | merged | feat/super-ontology (车道 AJ 并入) | 6c977a416 → 6c977a416 | — | — | Make the live tests follow D97 supersede handling |
| SO-B-S23-01 | D96 | merged | feat/super-ontology (车道 AJ 并入) | 45d9c59bf → 45d9c59bf | — | — | Ask the file classifier to keep chat logs, plans and test plans as records |
| SO-B-S23-02 | D96 | merged | feat/super-ontology (车道 AJ 并入) | cb46307d2 → cb46307d2 | — | — | Admit the model's mention of a Term a rule already defined |
| SO-B-S23-04 | D96 | merged | feat/super-ontology (车道 AJ 并入) | 8a73fc12e → 8a73fc12e | — | — | Pin the prompt version and the recorded keys after the AN merge |
| SO-B-S23-05 | D96 | merged | feat/super-ontology (车道 AJ 并入) | 32d50bff4 → 32d50bff4 | — | — | Settle a cross-language pair by an exact alias or a unique pinyin |
| SO-B-S23-06 | D96 | merged | feat/super-ontology (车道 AJ 并入) | 2c8254f67 → 2c8254f67 | — | — | Publish a verified about link to a project or an organisation |
| SO-B-S23-07 | D96 | merged | feat/super-ontology (车道 AJ 并入) | f76c489b1 → f76c489b1 | — | — | Limit the verified about link to projects and organisations a rule placed |

## 签收

| 闭环 | 签收提交 | OO sha | Puppyone sha | 证据 | 日期 |
|---|---|---|---|---|---|
| L8 | SO-SIGN-L8（OO 侧） | 见标签 | Desktop 阶段二 | testing/super-ontology/evidence/L8/ | 2026-11-18 |
| L4 | SO-SIGN-L4（OO 侧） | 见标签 | Desktop 阶段二 | testing/super-ontology/evidence/L4/ | 2026-11-18 |
| L3 | SO-SIGN-L3（OO 侧） | 见标签 | Desktop 阶段二 | testing/super-ontology/evidence/L3/ | 2026-11-18 |
| L6 | SO-SIGN-L6（OO 侧） | 见标签 | Desktop 阶段二 | testing/super-ontology/evidence/L6/ | 2026-11-18 |
| L5 | SO-SIGN-L5（OO 侧） | 见标签 | Desktop 阶段二 | testing/super-ontology/evidence/L5/ | 2026-11-18 |
| L2 | SO-SIGN-L2（OO 侧） | 见标签 | Desktop 阶段二 | testing/super-ontology/evidence/L2/ | 2026-11-17 |
| L1 | SO-SIGN-L1 | 92011a799141 | 530ed9b7f8a9 | testing/super-ontology/evidence/L1/ | 2026-11-17 |
