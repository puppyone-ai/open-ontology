# Super Ontology 台账

按 50 §9 维护：每张卡一行，每个闭环签收追加一行。状态只允许 todo、in_progress、review、merged、signed、dropped。主线程独占本文件。

| 卡号 | 闭环 | 状态 | 分支 | 提交（首 → 末） | PR | 证据 | 备注 |
|---|---|---|---|---|---|---|---|
| SO-SIGN-L2 | L2 | signed | feat/super-ontology (主线程) | 签收提交 | — | testing/super-ontology/evidence/L2/ | OO 侧（D80 阶段一）；标签 super-ontology/L2（本地） |
| SO-SIGN-L7 | L7 | review | so/sign-v2 | 证据提交（未签收） | — | testing/super-ontology/evidence/L7/ | OO 侧证据已记录，未签收：真实 OSS/COS/R2 桶与云队列需 owner 凭证（验收 1 未做）；MNS 签名、COS 事件格式未确认 |
| SO-SIGN-L5 | L5 | signed | so/sign-v2 | 签收提交 | — | testing/super-ontology/evidence/L5/ | OO 侧（D80 阶段一）；独立复核 42/42；修订模式视图与 editor 写入在阶段二；标签 super-ontology/L5（本地） |
| SO-SIGN-L3 | L3 | review | so/sign-v2 | 证据提交（未签收） | — | testing/super-ontology/evidence/L3/ | OO 侧证据已记录，未签收：S3 格子（RustFS、MinIO）、download、s3_rustfs 未独立重跑（固定镜像不在本机，拉取需 owner 批准） |
| SO-SIGN-L1 | L1 | signed | feat/super-ontology (主线程) | 签收提交 | — | testing/super-ontology/evidence/L1/ | 标签 super-ontology/L1（本地） |
| SO-SIGN-L4 | L4 | review | so/sign-v2 | 证据提交（未签收） | — | testing/super-ontology/evidence/L4/ | OO 侧证据已记录，未签收：conformance_share（FM6）在全量 live 套件负载下于 4cee27056 失败 1 次（单独通过），根因修复进行中；CLI 验收 58/58 |
| SO-SIGN-L6 | L6 | signed | so/sign-v2 | 签收提交 | — | testing/super-ontology/evidence/L6/ | OO 侧（D80 阶段一）；独立复核 CLI 36/36、e2e 28/28；ASR 选型与许可记录于 supply-chain/media-asr-model-reviews.json；macOS 宿主机解码与 puppyone share 在阶段二；标签 super-ontology/L6（本地） |
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
| SO-B-S7-01 | L5 | merged | feat/super-ontology (车道 B 分支并入) | 93276ee08 → 27af5ace2 | — | evidence/L5/ | docs-api 1.3 的 Suggest 接口与示例 |
| SO-B-S7-02 | L5 | merged | feat/super-ontology (车道 B 分支并入) | 9804a7f10 → 9804a7f10 | — | evidence/L5/ | suggest/ 目录收集与 oo docs suggestion |
| SO-B-S7-03 | L5 | merged | feat/super-ontology (车道 B 分支并入) | 22f16218e → cfbe78bea | — | evidence/L5/ | 建议记录、四条影响规则、存储与日摘要；PG 实测 |
| SO-B-S7-04 | L5 | merged | feat/super-ontology (车道 B 分支并入) | 06c5041ab → 06c5041ab | — | evidence/L5/ | MCP suggest、context_check 工具与 share updates 资源 |
| SO-B-S7-05 | L5 | merged | feat/super-ontology (车道 B 分支并入) | 8f64feffa → 8f64feffa | — | evidence/L5/ | suggester、editor Share 的 suggest:write 作用域 |
| SO-B-S7-06 | L5 | merged | feat/super-ontology (车道 B 分支并入) | ad0c3fbc0 → 7e510090d | — | evidence/L5/ | 代理接口列出 suggest 工具；源基线 revision 33 |
| SO-B-S8-02 | L5 | merged | feat/super-ontology (车道 B 分支并入) | 7836d9a65 → ef30a3341 | — | evidence/L5/ | L5 Suggest 端到端脚本与证据 |
| SO-B-S10-04 | L6 | merged | feat/super-ontology (车道 B 分支并入) | 808238251 → dc3591c59 | — | evidence/L6/ | media-parser 两处缺陷修复；L6 余项（A8、长媒体、OCR 子集、包滞后）并入 |
| SO-B-S11-01 | L7 | merged | feat/super-ontology (车道 B 分支并入) | c3d33649b → 7d24d670a | — | evidence/L7/ | oss、cos、r2 Drive 走 S3 兼容路径；连接器特性开关 |
| SO-B-S11-02 | L7 | merged | feat/super-ontology (车道 B 分支并入) | 908c8c781 → 4c01c56fe | — | evidence/L7/ | 桶事件通知转变更提示；docs-api 1.4 |
| SO-B-S11-03 | L7 | merged | feat/super-ontology (车道 B 分支并入) | 5d49d9649 → 5d49d9649 | — | evidence/L7/ | Drive 访问策略在入库与可见图中生效 |
| SO-B-S11-04 | L7 | merged | feat/super-ontology (车道 B 分支并入) | 12c8c1d6b → 8532d1d0e | — | evidence/L7/ | 已发布文档实体投影到 Agent DB 并关联行；投影迁移 |
| SO-B-S12-01 | L7 | merged | feat/super-ontology (车道 B 分支并入) | 28f63af34 → 044983b7d | — | evidence/L7/ | 云入口、共享审计、事件提示路由；审计迁移 |
| SO-B-S12-02 | L7 | merged | feat/super-ontology (车道 B 分支并入) | f3321e53e → f2d36e157 | — | evidence/L7/ | --remote 挂载远端 OO serve；L7 端到端脚本 |
| SO-B-S13-01 | L8 | merged | feat/super-ontology (车道 C 分支并入) | ac408d725 → c30c203d2 | — | testing/super-ontology/evidence/L8/ | 可选 pgvector 迁移 0144；服务端带 pgvector 时安装向量表 |
| SO-B-S13-02 | L8 | merged | feat/super-ontology (车道 C 分支并入) | 43e47007a → 43e47007a | — | testing/super-ontology/evidence/L8/ | 可选本地向量：BRIEF 其他相关与 resolve 召回 |
| SO-B-S13-03 | L8 | merged | feat/super-ontology (车道 C 分支并入) | cef94147a → 445d2700b | — | testing/super-ontology/evidence/L8/ | 1 万文件规模基准 |
| SO-B-S14-01 | L8 | merged | feat/super-ontology (车道 C 分支并入) | 7c02efdac → 7c02efdac | — | testing/super-ontology/evidence/L8/ | 无 Docker 运行时评审与原型测量（ADR-S14） |

## 签收

| 闭环 | 签收提交 | OO sha | Puppyone sha | 证据 | 日期 |
|---|---|---|---|---|---|
| L5 | SO-SIGN-L5（OO 侧） | 见标签 | Desktop 阶段二 | testing/super-ontology/evidence/L5/ | 2026-11-18 |
| L6 | SO-SIGN-L6（OO 侧） | 见标签 | Desktop 阶段二 | testing/super-ontology/evidence/L6/ | 2026-11-18 |
| L2 | SO-SIGN-L2（OO 侧） | 见标签 | Desktop 阶段二 | testing/super-ontology/evidence/L2/ | 2026-11-17 |
| L1 | SO-SIGN-L1 | 92011a799141 | 530ed9b7f8a9 | testing/super-ontology/evidence/L1/ | 2026-11-17 |
