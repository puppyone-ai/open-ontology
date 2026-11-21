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
| SO-SIGN-L4 | L4 | signed | feat/super-ontology (主线程) | 签收提交 | — | testing/super-ontology/evidence/L4/ | OO 侧证据已记录，未签收：conformance_share（FM6）在全量 live 套件负载下于 4cee27056 失败 1 次（单独通过），根因修复进行中；CLI 验收 58/58 |
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
| SO-B-S16-01 | D83 | merged | feat/super-ontology (WP1 分支并入) | 7b2f868dd → 93f9b597c | — | — | oo docs upload/wait 与托管本地 Drive |
| SO-B-S16-02 | D83 | merged | feat/super-ontology (WP1 分支并入) | 5fb887bf9 → 5fb887bf9 | — | — | oo source add/ls/show/rm/test：文件夹、桶、数据库来源，凭证 0600 引用 |
| SO-B-S16-03 | D83 | merged | feat/super-ontology (WP1 分支并入) | 8073b9853 → 8073b9853 | — | — | 原先只有 API 的命令（模型、版本、约束、审计等） |
| SO-B-S16-04 | D83 | merged | feat/super-ontology (WP1 分支并入) | 1a7ad8a2a → 1a7ad8a2a | — | — | oo serve --runtime auto|compose|embedded|host 与内嵌解析 sidecar |
| SO-B-S16-05 | D83 | merged | feat/super-ontology (WP1 分支并入) | c25304a5d → 6fe7c5dc1 | — | — | WP1 只用 CLI 的端到端脚本 |
| SO-B-S16-06 | D83 | merged | feat/super-ontology (WP1 分支并入) | 75b68a54a → bf5df0223 | — | — | 数据库来源登记与 upload 自动启动 serve |
| SO-B-S16-07 | D83 | merged | feat/super-ontology (WP1b 分支并入) | cc2321e47 → 056d7653a | — | — | 引擎镜像带 objectstore-s3（oo-docs-engine:dev-s3）、host.docker.internal、oo source link |
| SO-B-S18-05 | D83 | merged | feat/super-ontology (WP3b 分支并入) | 37ade531f → 37ade531f | — | — | Add docs-api 1.9 for full text search over facts and blocks |
| SO-B-S18-06 | D83 | merged | feat/super-ontology (WP3b 分支并入) | be054d583 → be054d583 | — | — | Serve full text search over facts and blocks from the docs engine |
| SO-B-S18-07 | D83 | merged | feat/super-ontology (WP3b 分支并入) | 591eed52c → 591eed52c | — | — | Search facts and blocks through the unified ontology search |
| SO-B-S18-08 | D83 | merged | feat/super-ontology (WP3b 分支并入) | ce5d422e6 → ce5d422e6 | — | — | Make the unified search fit the Gateway SQL guard |
| SO-B-S18-09 | D83 | merged | feat/super-ontology (WP3b 分支并入) | ab7ee3d1e → 5d5abe689 | — | — | Run the CLI-first end to end against the real OO Gateway |
| SO-B-S16-08 | D83 | merged | feat/super-ontology (车道 F 分支并入) | 89a4ab519 → 89a4ab519 | — | — | Install the Docker-free document parser runtime from oo itself |
| SO-B-S16-09 | D83 | merged | feat/super-ontology (车道 F 分支并入) | 4804931b7 → 4804931b7 | — | — | Start the daemon from the first oo source add on a fresh HOME |
| SO-B-S16-10 | D83 | merged | feat/super-ontology (车道 F 分支并入) | d6012a50e → d6012a50e | — | — | Lock the rebuilt ocr-parser image with the text-layer fill |
| SO-B-S16-11 | D83 | merged | feat/super-ontology (车道 F 分支并入) | 5224a5bb9 → 5224a5bb9 | — | — | Prove docx and pdf publish from a fresh HOME without Docker |
| SO-B-S16-12 | D83 | merged | feat/super-ontology (车道 F 分支并入) | ba022bf4c → ba022bf4c | — | — | Test the CLI-only paths no script covered |
| SO-B-S16-13 | D83 | merged | feat/super-ontology (车道 F 分支并入) | 6d88bafef → 63cdef04d | — | — | Record the F1 evidence and refresh the OCR subset measurement |
| SO-B-S18-01 | D83 | merged | feat/super-ontology (WP3 分支并入) | a0c7e4863 → a0c7e4863 | — | — | docs-api 1.7：统一本体查询接口 |
| SO-B-S18-02 | D83 | merged | feat/super-ontology (WP3 分支并入) | 53c983147 → 04e772972 | — | — | /v1/ontology/* 代理、oo ontology search/entity/query/sources、oo agent token |
| SO-B-S18-03 | D83 | merged | feat/super-ontology (WP3 分支并入) | c213e6aeb → c213e6aeb | — | — | MCP 统一工具与 oo mcp --stdio |
| SO-B-S18-04 | D83 | merged | feat/super-ontology (WP3 分支并入) | 414a465b6 → 4462e873a | — | — | 只用 CLI 的端到端脚本（文档与数据库两边、负例） |
| SO-B-S17-01 | D83 | merged | feat/super-ontology (WP2 分支并入) | 77c2e4e87 → 88463bde4 | — | — | 投影作业：已发布文档实体与事实增量投影到共享实体命名空间，带出处 |
| SO-B-S17-02 | D83 | merged | feat/super-ontology (WP2 分支并入) | 45626acf9 → a7c00590c | — | — | ooa_ Agent 令牌、scope 校验、审计、吊销；链接路由；迁移 0150、0151 |
| SO-B-S17-03 | D83 | merged | feat/super-ontology (WP2 分支并入) | 766f2900c → 766f2900c | — | — | 装入器接入 0150–0152 |
| SO-B-S17-04 | D83 | merged | feat/super-ontology (WP2 分支并入) | f66b870ec → f66b870ec | — | — | docs-api 1.6：令牌与链接路由 |
| SO-B-S17-05 | D83 | merged | feat/super-ontology (WP2 分支并入) | 9cf41fce6 → 26878a66c | — | — | 令牌、吊销、投影增量与跨源链接的测试 |
| SO-B-S14-02 | L8 | merged | feat/super-ontology (车道 G 分支并入) | 67e555dcf → d656d8923 | — | — | Install a pinned FFmpeg-free OpenCV wheel and a pinned uv in the Docker-free runtime |
| SO-B-S14-03 | L8 | merged | feat/super-ontology (车道 G 分支并入) | e37e09578 → e37e09578 | — | — | Prove the Docker-free runtime ships no FFmpeg from a fresh HOME |
| SO-B-S14-04 | L8 | merged | feat/super-ontology (车道 G 分支并入) | b0467c48b → b0467c48b | — | — | Build the release engine with objectstore-s3 beside oo |
| SO-B-S14-05 | L8 | merged | feat/super-ontology (车道 G 分支并入) | 3ebaa1319 → 3ebaa1319 | — | — | Record the Docker-free distribution decisions in the ADR |
| SO-B-S14-06 | L8 | merged | feat/super-ontology (车道 G 分支并入) | 4e70ce2e9 → 4e70ce2e9 | — | — | Pass the pinned runtime artifacts through the WP1 CLI input run |
| SO-B-S14-07 | L8 | merged | feat/super-ontology (车道 G 分支并入) | 27076f436 → 27076f436 | — | — | Record the lane G WP1 and CLI-only matrix runs |
| SO-B-S15-02 | D84 | merged | feat/super-ontology (打榜车道并入) | 0b993fe5d → 0b993fe5d | — | testing/super-ontology/bench/ | Add the bm25/1 lexical retrieval channel to oo-docs |
| SO-B-S15-11 | D84 | merged | feat/super-ontology (打榜车道并入) | d0c7f5288 → 1e02d8a11 | — | testing/super-ontology/bench/ | Add the walk/1 anchored graph ranking channel to oo-docs |
| SO-B-S15-12 | D84 | merged | feat/super-ontology (打榜车道并入) | dffaafefb → dffaafefb | — | testing/super-ontology/bench/ | Record the verified STaRK and GraphRAG-Bench leaderboard rules |
| SO-B-S15-13 | D84 | merged | feat/super-ontology (打榜车道并入) | 5f2890577 → 0cc970163 | — | testing/super-ontology/bench/ | Drive STaRK knowledge bases through the oo-docs engine |
| SO-B-S15-14 | D84 | merged | feat/super-ontology (打榜车道并入) | 2ab7fa33f → 2ab7fa33f | — | testing/super-ontology/bench/ | Weight graph anchors by specificity in walk/2 |
| SO-B-S15-15 | D84 | merged | feat/super-ontology (打榜车道并入) | 2dbf396c2 → a1080d804 | — | testing/super-ontology/bench/ | Drive GraphRAG-Bench corpora through the oo-docs engine |
| SO-B-S15-16 | D84 | merged | feat/super-ontology (打榜车道并入) | f49aca78d → a133e3f6f | — | testing/super-ontology/bench/ | Freeze the STaRK configs and record the dev runs |
| SO-B-S15-01 | D84 | merged | feat/super-ontology (打榜车道 B1 并入) | 5ac110e52 → 5ac110e52 | — | testing/super-ontology/bench/longmemeval-v2/ | Record the verified LongMemEval-V2 leaderboard rules |
| SO-B-S15-03 | D84 | merged | feat/super-ontology (打榜车道 B1 并入) | 0a33fe389 → 0a33fe389 | — | testing/super-ontology/bench/longmemeval-v2/ | Drive LongMemEval-V2 trajectories through the oo-docs engine |
| SO-B-S15-04 | D84 | merged | feat/super-ontology (打榜车道 B1 并入) | 1e89a87fa → 1e89a87fa | — | testing/super-ontology/bench/longmemeval-v2/ | Run the official LongMemEval-V2 harness with the OO memory |
| SO-B-S15-05 | D84 | merged | feat/super-ontology (打榜车道 B1 并入) | 13dd028e9 → 13dd028e9 | — | testing/super-ontology/bench/longmemeval-v2/ | Tune trajectory evidence: element ids, gaps, coverage, phrases |
| SO-B-S15-06 | D84 | merged | feat/super-ontology (打榜车道 B1 并入) | 190b222a2 → 190b222a2 | — | testing/super-ontology/bench/longmemeval-v2/ | Report exact-text hits and misses for quoted phrases |
| SO-B-S15-07 | D84 | merged | feat/super-ontology (打榜车道 B1 并入) | 675fe7a89 → 675fe7a89 | — | testing/super-ontology/bench/longmemeval-v2/ | Add the LongMemEval-V2 dev run logs |
| SO-B-S15-08 | D84 | merged | feat/super-ontology (打榜车道 B1 并入) | bd9e46479 → 77198369f | — | testing/super-ontology/bench/longmemeval-v2/ | Record the LongMemEval-V2 run setup, configs and dev results |
| SO-B-S16-14 | D83 | merged | feat/super-ontology (App 车道 A 并入) | 928cb5487 → 928cb5487 | — | apps/super-ontology-desktop/ | Embed the compose deploy files in oo and write them to the state dir on first use |
| SO-F-S19-01 | D85 | merged | feat/super-ontology (App 车道 A 并入) | 83e0e752b → 83e0e752b | — | apps/super-ontology-desktop/ | Define the preload interface of the standalone Super Ontology app |
| SO-F-S19-02 | D85 | merged | feat/super-ontology (App 车道 A 并入) | 98f711d5d → 98f711d5d | — | apps/super-ontology-desktop/ | Add the Super Ontology app package, build config and dependency records |
| SO-F-S19-03 | D85 | merged | feat/super-ontology (App 车道 A 并入) | f8f460048 → f8f460048 | — | apps/super-ontology-desktop/ | Add the main process, preload and engine bootstrap of the Super Ontology app |
| SO-F-S19-04 | D85 | merged | feat/super-ontology (App 车道 A 并入) | f2f07cca7 → f2f07cca7 | — | apps/super-ontology-desktop/ | Add the placeholder icon, the Apache attribution and the notice generation |
| SO-F-S19-05 | D85 | merged | feat/super-ontology (App 车道 A 并入) | 8bce99757 → 081cd185c | — | apps/super-ontology-desktop/ | Add the owner-triggered workflow that builds the Super Ontology DMG |
| SO-F-S19-06 | D85 | merged | feat/super-ontology (App 车道 A 并入) | 2645cefa6 → e5c0062a2 | — | apps/super-ontology-desktop/ | Bundle the release oo with its docs engine in the Super Ontology app |
| SO-F-S19-21 | D85 | merged | feat/super-ontology (App 车道 C 并入) | 1ba30642d → 061afee5c | — | apps/super-ontology-desktop/ | Generate the app contract modules straight from contracts |
| SO-F-S19-22 | D85 | merged | feat/super-ontology (App 车道 C 并入) | 26ae0586a → 26ae0586a | — | apps/super-ontology-desktop/ | Port the main-process and contract test suites with the fake engine |
| SO-F-S19-23 | D85 | merged | feat/super-ontology (App 车道 C 并入) | 1feab20f8 → 1feab20f8 | — | apps/super-ontology-desktop/ | Port the renderer data and service layer unit suites |
| SO-F-S19-24 | D85 | merged | feat/super-ontology (App 车道 C 并入) | 6b108c2a5 → a83942ec2 | — | apps/super-ontology-desktop/ | List the test ids the renderer and the T1 scenarios agree on |
| SO-F-S19-10 | D85 | merged | feat/super-ontology (App 车道并入) | c1ec14759 → c1ec14759 | — | apps/super-ontology-desktop/ | Add the Super Ontology design system package |
| SO-F-S19-11 | D85 | merged | feat/super-ontology (App 车道并入) | 414f6fa42 → 414f6fa42 | — | apps/super-ontology-desktop/ | Add the Super Ontology app data and service layer |
| SO-F-S19-12 | D85 | merged | feat/super-ontology (App 车道并入) | bf2200e95 → bf2200e95 | — | apps/super-ontology-desktop/ | Add the app shell and the Home screen |
| SO-F-S19-13 | D85 | merged | feat/super-ontology (App 车道并入) | 365879373 → 365879373 | — | apps/super-ontology-desktop/ | Add the Sources screen |
| SO-F-S19-14 | D85 | merged | feat/super-ontology (App 车道并入) | 9e1425054 → 9e1425054 | — | apps/super-ontology-desktop/ | Add the Ontology and Agents screens |
| SO-F-S19-15 | D85 | merged | feat/super-ontology (App 车道并入) | ed3b88cbe → ed3b88cbe | — | apps/super-ontology-desktop/ | Add Settings and the five-step first run |
| SO-F-S19-32 | D85 | merged | feat/super-ontology (App 车道并入) | 7964c606e → 7964c606e | — | apps/super-ontology-desktop/ | Wire the preload channels into the renderer and carry the test ids on its screens |
| SO-F-S19-33 | D85 | merged | feat/super-ontology (App 车道并入) | 4ac16c81a → 4ac16c81a | — | apps/super-ontology-desktop/ | Polish the screens after the review of the real window |
| SO-F-S19-34 | D85 | merged | feat/super-ontology (App 车道并入) | a93cb23ee → fcf126c88 | — | apps/super-ontology-desktop/ | Refresh the third-party notice for the renderer dependencies |
| SO-F-S19-41 | D86 | merged | feat/super-ontology (App 车道 E 并入) | f1e45f89e → f1e45f89e | — | apps/super-ontology-desktop/ | Fix stale Home counts, add database sources, eight-language catalog and whole-pixel layout |
| SO-F-S19-44 | D86 | merged | feat/super-ontology (App 车道 E 并入) | 908039fac → 908039fac | — | apps/super-ontology-desktop/ | Leave database sources to the connector lane and keep the first run steady |
| SO-F-S19-45 | D86 | merged | feat/super-ontology (App 车道 E 并入) | 383b34b0d → 383b34b0d | — | apps/super-ontology-desktop/ | Fix the T2 source seed key and the T1 scenario list |
| SO-F-S19-46 | D86 | merged | feat/super-ontology (App 车道 E 并入) | ead08af90 → ead08af90 | — | apps/super-ontology-desktop/ | Make T1 fail when a scenario fails, and add the database and visual suites |
| SO-F-S19-47 | D86 | merged | feat/super-ontology (App 车道 E 并入) | 72bedef6b → bc4f35ce3 | — | apps/super-ontology-desktop/ | Record the app's npm dependencies and list every bundled package in the notice |
| SO-F-S19-60 | D88 | merged | feat/super-ontology (App 车道 G 并入) | 5305be082 → 08321d3ce | — | apps/super-ontology-desktop/ | Add the living ontology atlas to the app (D88) |
| SO-F-S19-61 | D88 | merged | feat/super-ontology (App 车道 G 并入) | 96e9c43ba → 96e9c43ba | — | apps/super-ontology-desktop/ | Refresh the third-party notice for the layout dependency |
| SO-B-S15-09 | D84 | merged | feat/super-ontology (打榜车道 B1 并入) | fde636761 → fde636761 | — | testing/super-ontology/bench/longmemeval-v2/ | Show complete, compact trees for distinct pages |
| SO-B-S15-10 | D84 | merged | feat/super-ontology (打榜车道 B1 并入) | d19477961 → d19477961 | — | testing/super-ontology/bench/longmemeval-v2/ | Retrieve every page a question compares |
| SO-F-S19-62 | D88 | merged | feat/super-ontology (App 车道 G 并入) | 541239f88 → 541239f88 | — | apps/super-ontology-desktop/ | Give the atlas one calm palette, quieter links, clearer labels and a source-only split |
| SO-B-S19-50 | D88 | merged | feat/super-ontology (App 车道并入) | 7199341b2 → 7199341b2 | — | apps/super-ontology-desktop/ | Accept every doctor-supported database kind in oo source add and test all resolved address |
| SO-F-S19-50 | D88 | merged | feat/super-ontology (App 车道并入) | f17b13b35 → f17b13b35 | — | apps/super-ontology-desktop/ | Add the CLI parity bridge: database sources, pipeline jobs, Gateway queries, diagnostics |
| SO-F-S19-51 | D88 | merged | feat/super-ontology (App 车道并入) | 1e08be37b → 1e08be37b | — | apps/super-ontology-desktop/ | Add database sources, Query, ontology YAML files and Diagnostics to the app |
| SO-F-S19-52 | D88 | merged | feat/super-ontology (App 车道并入) | 1cff3fe64 → 1cff3fe64 | — | apps/super-ontology-desktop/ | Translate the CLI parity strings into the six other app languages |
| SO-F-S19-53 | D88 | merged | feat/super-ontology (App 车道并入) | 722e6020f → 00c8f9ad3 | — | apps/super-ontology-desktop/ | Add T1 scenarios, visual tests and the parity matrix for the CLI parity screens |
| SO-F-S19-54 | D88 | merged | feat/super-ontology (App 车道并入) | a5dae044f → a5dae044f | — | apps/super-ontology-desktop/ | Repair the merge with the atlas: Ontology tabs, app state, T1 order and test ids |
| SO-F-S19-63 | D88 | merged | feat/super-ontology (App 车道并入) | 681bae617 → 681bae617 | — | apps/super-ontology-desktop/ | Keep group labels clear of members and tell the kinds of source apart |
| SO-B-S18-10 | D88 | merged | feat/super-ontology (车道 L 并入) | 31de80533 → 31de80533 | — | — | Add GET /v1/ontology/schema and oo ontology schema |
| SO-B-S18-11 | D88 | merged | feat/super-ontology (车道 L 并入) | 45ec7e0e7 → 43f287894 | — | — | Test the ontology schema route on the real Gateway |
| SO-F-S19-70 | D88 | merged | feat/super-ontology (车道 L 并入) | 25b385a39 → 25b385a39 | — | — | Read the atlas database side from the ontology schema channel |
| SO-F-S19-71 | D88 | merged | feat/super-ontology (车道 L 并入) | 8b9c37867 → 8b9c37867 | — | — | Make the atlas T1 settle on the fake and on the real engine |
| SO-B-S19-51 | D88 | merged | feat/super-ontology (车道 K 并入) | d9e10668d → d9e10668d | — | — | Probe the MySQL-wire, analytic and openGauss families in oo doctor |
| SO-B-S19-52 | D88 | merged | feat/super-ontology (车道 K 并入) | 85131044d → 85131044d | — | — | Add oo source add for every remaining database kind |
| SO-B-S19-53 | D88 | merged | feat/super-ontology (车道 K 并入) | 7abe56c5d → 7abe56c5d | — | — | Keep the kind listing for tests only |
| SO-B-S19-54 | D88 | merged | feat/super-ontology (车道 K 并入) | 2958ec10f → 56fe206df | — | — | Pin the locally built arm64 lite runtime image |
| SO-B-S19-56 | D88 | merged | feat/super-ontology (车道 K 并入) | 82337bb28 → 82337bb28 | — | — | Scale the Builder model-call budget with the schema |
| SO-B-S19-57 | D88 | merged | feat/super-ontology (车道 K 并入) | f78fdb6db → f78fdb6db | — | — | Let a build review file carry the one confirmation and its edge decisions |
| SO-B-S19-58 | D88 | merged | feat/super-ontology (车道 K 并入) | 6a76a7292 → 6a76a7292 | — | — | Accept a non-stream reply that the gateway labels as an event stream |
| SO-B-S19-59 | D88 | merged | feat/super-ontology (车道 K 并入) | 60b4973f8 → 60b4973f8 | — | — | Escape the quotes in the build review help text |
| SO-B-S19-60 | D88 | merged | feat/super-ontology (车道 K 并入) | 9a0ab26be → 9a0ab26be | — | — | Export the owner's reviewed ontology as YAML |
| SO-B-S19-61 | D88 | merged | feat/super-ontology (车道 K 并入) | 3131990b0 → 3131990b0 | — | — | Let a build leave the owner's CLI profiles alone |
| SO-B-S19-62 | D88 | merged | feat/super-ontology (车道 K 并入) | 3797e7b01 → 3797e7b01 | — | — | Make the build review test write private files and format the review module |
| SO-B-S19-63 | D88 | merged | feat/super-ontology (车道 K 并入) | fb836f3f6 → fb836f3f6 | — | — | Say what is wrong when a review file cannot be read |
| SO-B-S19-64 | D88 | merged | feat/super-ontology (车道 K 并入) | 940485148 → 940485148 | — | — | Test the review file error messages |
| SO-B-S19-65 | D88 | merged | feat/super-ontology (车道 K 并入) | 01ec59905 → 01ec59905 | — | — | Pin the arm64 lite runtime built from the integration tip |
| SO-F-S19-80 | D85 | merged | feat/super-ontology (App 车道 M 并入) | 69692efd4 → 7a875a53f | — | apps/super-ontology-desktop/ | Make the T2 runner address the launched process and the native open panel |
| SO-B-S19-66 | D88 | merged | feat/super-ontology (车道 K 并入) | 33b7cf043 → 33b7cf043 | — | — | Show why the load, candidate and builder stages failed |
| SO-B-S19-67 | D88 | merged | feat/super-ontology (车道 K 并入) | 6f4398f36 → 6f4398f36 | — | — | Resolve symlinked directories in file secret references |
| SO-B-S19-68 | D88 | merged | feat/super-ontology (车道 K 并入) | 3331df782 → 3331df782 | — | — | Strip one trailing newline from secrets |
| SO-B-S19-69 | D88 | merged | feat/super-ontology (车道 K 并入) | cf7873d31 → cf7873d31 | — | — | Say which docker command failed and why |
| SO-B-S19-70 | D88 | merged | feat/super-ontology (车道 K 并入) | 0347499de → 0347499de | — | — | Give the helper container the file secrets a source names |
| SO-B-S19-71 | D88 | merged | feat/super-ontology (车道 K 并入) | 527f6f39c → 527f6f39c | — | — | Keep multi-line secrets exact when stripping the newline |
| SO-B-S19-72 | D88 | merged | feat/super-ontology (车道 K 并入) | 839822527 → 839822527 | — | — | Pin the arm64 lite runtime with the secret and error fixes |
| SO-B-S20-10 | D83 | merged | feat/super-ontology (车道 V 并入) | 2a9aeb407 → 2a9aeb407 | — | — | Fix a 45-question agent QA set with gold answers before any agent run |
| SO-B-S20-11 | D83 | merged | feat/super-ontology (车道 V 并入) | af6206937 → af6206937 | — | — | Read --model as a value in oo docs model set |
| SO-B-S20-12 | D83 | merged | feat/super-ontology (车道 V 并入) | 6afa1aae9 → 6afa1aae9 | — | — | Show the Agent commands in oo ontology --help and the top-level help |
| SO-B-S20-13 | D83 | merged | feat/super-ontology (车道 V 并入) | ac44a2f2a → ac44a2f2a | — | — | Add a tool-calling agent harness that scores answers over the oo CLI |
| SO-B-S20-14 | D83 | merged | feat/super-ontology (车道 V 并入) | 9298fd5b5 → 9298fd5b5 | — | — | Mount host paths into the engine only for the control token |
| SO-B-S20-15 | D83 | merged | feat/super-ontology (车道 V 并入) | 6c258b580 → 6c258b580 | — | — | Say in the ontology help that search matches text literally |
| SO-B-S20-16 | D83 | merged | feat/super-ontology (车道 V 并入) | 98496e49f → 98496e49f | — | — | Tune the QA harness on the dev split and accept Gateway query ids as citations |
| SO-B-S20-17 | D83 | merged | feat/super-ontology (车道 V 并入) | 913b9bcfa → 913b9bcfa | — | — | Run the same agent over oo mcp --stdio in the QA harness |
| SO-B-S20-18 | D83 | merged | feat/super-ontology (车道 V 并入) | 5c41e35fc → 5c41e35fc | — | — | Pass the Gateway's message and hint through oo ontology query |
| SO-B-S20-19 | D83 | merged | feat/super-ontology (车道 V 并入) | 7ddf70700 → 7ddf70700 | — | — | Record the agent QA runs and the token safety checks |
| SO-B-S20-20 | D83 | merged | feat/super-ontology (车道 X 并入) | 94b7fc5f8 → 94b7fc5f8 | — | — | Mount the Agent DB secret file into the compose engine on bind |
| SO-B-S20-21 | D83 | merged | feat/super-ontology (车道 X 并入) | f80d3c34d → f80d3c34d | — | — | Deduplicate the relationships in oo ontology schema |
| SO-B-S20-22 | D83 | merged | feat/super-ontology (车道 X 并入) | d1119b459 → d1119b459 | — | — | Retry an OVERLOADED Gateway answer with backoff in oo serve |
| SO-B-S20-23 | D83 | merged | feat/super-ontology (车道 X 并入) | 99702df70 → 99702df70 | — | — | Ask extraction for a cross-language alias of each entity |
| SO-B-S20-24 | D83 | merged | feat/super-ontology (车道 X 并入) | bcae702c9 → bcae702c9 | — | — | Say how to try other forms when an ontology search is empty |
| SO-B-S20-29 | D83 | merged | feat/super-ontology (车道 X 并入，协调者合并与台账) | 890700b90 → c6fd4d9e4 | — | — | Merge lane X and add its ledger rows |
| SO-B-S20-30 | D83 | merged | feat/super-ontology (车道 AB 并入) | 0198b3229 → 0198b3229 | — | — | Add the acceptance folder generator |
| SO-B-S20-31 | D83 | merged | feat/super-ontology (车道 AB 并入) | bd8ebcc74 → bd8ebcc74 | — | — | Add the acceptance corpus and its frozen gold |
| SO-B-S20-32 | D83 | merged | feat/super-ontology (车道 AB 并入) | 05b280420 → 05b280420 | — | — | Start the OCR and media sidecars when their locked images are here |
| SO-B-S20-33 | D83 | merged | feat/super-ontology (车道 AB 并入) | 85242d089 → 85242d089 | — | — | Give the Latin text of the corpus PDFs a real glyph width |
| SO-B-S20-34 | D83 | merged | feat/super-ontology (车道 AB 并入) | 51a8c4717 → 51a8c4717 | — | — | Decode AAC and H.264 media on the Mac without a button |
| SO-B-S20-36 | D83 | merged | feat/super-ontology (车道 AB 并入) | 736afb840 → 736afb840 | — | — | Render dash lines of an Ask answer as a real list |
| SO-B-S20-38 | D83 | merged | feat/super-ontology (车道 AB 并入) | 38e6871fe → 38e6871fe | — | — | Wait 180 seconds for the model's answer, not 45 |
| SO-B-S20-39 | D83 | merged | feat/super-ontology (车道 AB 并入) | dacc89fa6 → 4a9ad4251 | — | — | Record the packaged acceptance runs |
| SO-B-S20-01 | D84 | merged | feat/super-ontology (车道 U 并入) | 617e0daaf → 617e0daaf | — | — | Let the model egress wait for a reasoning model |
| SO-B-S20-02 | D84 | merged | feat/super-ontology (车道 U 并入) | 90612ec65 → 90612ec65 | — | — | Drop the about-an-Event labels the contract cannot hold |
| SO-B-S20-03 | D84 | merged | feat/super-ontology (车道 U 并入) | 2f158e726 → 2f158e726 | — | — | Teach extract/3 the Event, Topic, Term and tried_and_failed boundaries |
| SO-B-S20-04 | D84 | merged | feat/super-ontology (车道 U 并入) | 603a88af9 → 603a88af9 | — | — | Name the fact state row of the eval diagnostics |
| SO-B-S20-06 | D84 | merged | feat/super-ontology (车道 U 并入) | 16d69be4d → 16d69be4d | — | — | Bump the merged prompt to extract/4 |
| SO-B-S20-07 | D84 | merged | feat/super-ontology (车道 U 并入) | b9affae49 → b9affae49 | — | — | Record the grok-4.7 quality measurement |
| SO-B-S20-40 | D84 | merged | feat/super-ontology (车道 AD 并入) | db02114a7 → db02114a7 | — | — | Log the wire latency of each model call for a measuring run |
| SO-B-S20-41 | D84 | merged | feat/super-ontology (车道 AD 并入) | f44537fd0 → f44537fd0 | — | — | Pass the model timeout and the latency log on to the engine of the L4 run |
| SO-B-S20-42 | D84 | merged | feat/super-ontology (车道 AD 并入) | 10705b90e → 10705b90e | — | — | Read a bilingual header as one column |
| SO-B-S20-43 | D84 | merged | feat/super-ontology (车道 AD 并入) | 1d62d409e → 1d62d409e | — | — | Tie documents by the amounts and dates they share |
| SO-B-S20-44 | D84 | merged | feat/super-ontology (车道 AD 并入) | 0463fd9b5 → 0463fd9b5 | — | — | Merge the same person named in two scripts |
| SO-B-S20-45 | D84 | merged | feat/super-ontology (车道 AD 并入) | ef335f2d0 → ef335f2d0 | — | — | Publish the rules facts of a file while its model call runs |
| SO-B-S20-46 | D84 | merged | feat/super-ontology (车道 AD 并入) | 9e7896981 → 9e7896981 | — | — | Suggest a model when a connection is set up |
| SO-B-S20-48 | D84 | merged | feat/super-ontology (车道 AD 并入) | 49eecc883 → 49eecc883 | — | — | Skip a malformed line of the latency log |
| SO-B-S20-49 | D84 | merged | feat/super-ontology (车道 AD 并入) | 7db5a2778 → 7db5a2778 | — | — | Let the model egress admit 120 calls a minute and make it configurable |
| SO-B-S20-50 | D84 | merged | feat/super-ontology (车道 AD 并入) | 987ba5891 → 987ba5891 | — | — | Give the early publish its own job key and fix the cross-language tests |
| SO-B-S20-51 | D84 | merged | feat/super-ontology (车道 AD 并入) | b6abab7c9 → b6abab7c9 | — | — | Default to eight parallel model extractions and the measured model |
| SO-B-S20-52 | D84 | merged | feat/super-ontology (车道 AD 并入) | 141336f7b → 141336f7b | — | — | Lock the engine image built from this tree |
| SO-B-S20-53 | D84 | merged | feat/super-ontology (车道 AD 并入) | 67a1e60ce → 67a1e60ce | — | — | Record the model comparison and the acceptance run of lane AD |
| SO-F-S19-81 | D85 | merged | feat/super-ontology (App 车道 N 并入) | 586ef4202 → 586ef4202 | — | apps/super-ontology-desktop/ | Make the T2 runner click, type and choose folders deterministically |
| SO-F-S19-82 | D85 | merged | feat/super-ontology (App 车道 N 并入) | 7f45363a3 → 7f45363a3 | — | apps/super-ontology-desktop/ | Correct the T2 scenarios against what the app does |
| SO-F-S19-83 | D85 | merged | feat/super-ontology (App 车道 N 并入) | 3b40e46cf → 3b40e46cf | — | apps/super-ontology-desktop/ | Keep the fake engine for the T1 scenarios that declare it and stop the packaged query run  |
| SO-F-S19-84 | D85 | merged | feat/super-ontology (App 车道 N 并入) | bade9b011 → e236da74a | — | apps/super-ontology-desktop/ | Pick a dialog's own button, scroll to a control below the fold and run the boot check in b |
| SO-F-S19-90 | D88 | merged | feat/super-ontology (App 车道 P 并入) | 23d72a061 → 23d72a061 | — | apps/super-ontology-desktop/ | Record a real Builder draft of a synthetic shop as test data |
| SO-F-S19-91 | D88 | merged | feat/super-ontology (App 车道 P 并入) | b065cf8a3 → b065cf8a3 | — | apps/super-ontology-desktop/ | Add all seventeen database kinds and the build review to the app |
| SO-F-S19-92 | D88 | merged | feat/super-ontology (App 车道 P 并入) | afa67b278 → 07596c575 | — | apps/super-ontology-desktop/ | Prove the database path from Add to Query in the app |
| SO-B-S16-15 | D85 | merged | feat/super-ontology (车道 R 并入) | 432654a0e → 432654a0e | — | — | Bundle every helper oo starts and check they match the build |
| SO-F-S19-93 | D85 | merged | feat/super-ontology (车道 R 并入) | 497aec1b3 → 497aec1b3 | — | — | Show a database-only atlas without a warning and draw links inside a group |
| SO-F-S19-94 | D85 | merged | feat/super-ontology (车道 R 并入) | 17659e06f → 17659e06f | — | — | Bundle every helper oo starts into the app and refuse a mismatched set |
| SO-F-S19-95 | D85 | merged | feat/super-ontology (车道 R 并入) | 4c7d6c951 → 3655c2490 | — | — | Let the live database scenario stop after the doctor |
| SO-F-S19-97 | D88 | merged | feat/super-ontology (App 车道 T 并入) | afbea4338 → afbea4338 | — | apps/super-ontology-desktop/ | Draw the atlas as precise growth: dots, squares, hull outlines, straight links |
| SO-F-S19-98 | D88 | merged | feat/super-ontology (App 车道 T 并入) | db9167a3e → db9167a3e | — | apps/super-ontology-desktop/ | Show the database tables and relationships in the atlas list and a version tick |
| SO-F-S19-99 | D88 | merged | feat/super-ontology (App 车道 T 并入) | ce2578c7e → 08ac1c00f | — | apps/super-ontology-desktop/ | Summarise cross-group links, cap mark size, pixel-grid canvas; single-database column layout |
| SO-F-S20-01 | D46 | merged | feat/super-ontology (App 车道 W 并入) | 855ba596a → 855ba596a | — | apps/super-ontology-desktop/ | Stop T2 screen recordings when a scenario ends and cap them |
| SO-F-S20-02 | D46 | merged | feat/super-ontology (App 车道 W 并入) | 3ba6e0d28 → 3ba6e0d28 | — | apps/super-ontology-desktop/ | Expose the freshness state and the change-ring count on the page |
| SO-F-S20-03 | D46 | merged | feat/super-ontology (App 车道 W 并入) | 8501c8570 → 8501c8570 | — | apps/super-ontology-desktop/ | Add the P07 auto-sync scenario and its measuring harness |
| SO-F-S20-04 | D46 | merged | feat/super-ontology (App 车道 W 并入) | 8afcde782 → 8afcde782 | — | apps/super-ontology-desktop/ | Re-check a held mass delete on the engine's own syncs and keep a lone file from tripping it |
| SO-F-S20-05 | D46 | merged | feat/super-ontology (App 车道 W 并入) | be83b9fbe → be83b9fbe | — | apps/super-ontology-desktop/ | Do the P07 drop through the upload panel and summarise its timings |
| SO-F-S20-06 | D46 | merged | feat/super-ontology (App 车道 W 并入) | 5e23677a2 → 5e23677a2 | — | apps/super-ontology-desktop/ | Fix uploads from the source page: pass the bridge real File objects |
| SO-F-S20-07 | D46 | merged | feat/super-ontology (App 车道 W 并入) | a0bfd16e6 → a0bfd16e6 | — | apps/super-ontology-desktop/ | Send a full sync when a changed path has vanished from disk |
| SO-F-S20-08 | D46 | merged | feat/super-ontology (App 车道 W 并入) | ba971a62e → ba971a62e | — | apps/super-ontology-desktop/ | Make P07 repeatable and exact about renames, deletes and the atlas |
| SO-F-S20-09 | D46 | merged | feat/super-ontology (App 车道 W 并入) | 7b9e1da77 → 7b9e1da77 | — | apps/super-ontology-desktop/ | Judge P07 evidence per file, retry lost clicks and report steps that were not run |
| SO-F-S20-20 | D85 | merged | feat/super-ontology (App 车道 Z 并入) | 381f83fe5 → 381f83fe5 | — | apps/super-ontology-desktop/ | List agent tokens with state=all so a revoked token shows as revoked |
| SO-F-S20-10 | D86 | merged | feat/super-ontology (App 车道 Y 并入) | ddfb582a3 → ddfb582a3 | — | apps/super-ontology-desktop/ | Move native strings to JSON catalogs and fill their gaps |
| SO-F-S20-11 | D86 | merged | feat/super-ontology (App 车道 Y 并入) | b4233cf6a → b4233cf6a | — | apps/super-ontology-desktop/ | Strengthen check:i18n and close the gaps it finds |
| SO-F-S20-12 | D86 | merged | feat/super-ontology (App 车道 Y 并入) | 32dc280d0 → 32dc280d0 | — | apps/super-ontology-desktop/ | Record the vocabulary shared with the sibling desktop app and test it |
| SO-F-S20-13 | D86 | merged | feat/super-ontology (App 车道 Y 并入) | 8eb7c6757 → 8eb7c6757 | — | apps/super-ontology-desktop/ | Show engine and CLI errors in the language of the app, with the code on a details line |
| SO-F-S20-14 | D86 | merged | feat/super-ontology (App 车道 Y 并入) | 743d5a885 → 743d5a885 | — | apps/super-ontology-desktop/ | Bring the language setting up to the sibling app's behaviour |
| SO-F-S20-15 | D86 | merged | feat/super-ontology (App 车道 Y 并入) | 073bdba54 → 073bdba54 | — | apps/super-ontology-desktop/ | Keep the CLI cause first and settle terms after the visual review |
| SO-F-S20-16 | D86 | merged | feat/super-ontology (App 车道 Y 并入) | 912375045 → 912375045 | — | apps/super-ontology-desktop/ | Add a locale screenshot tool and a language switch for the T1 runs |
| SO-F-S20-30 | D46 | merged | feat/super-ontology (App 车道 W 并入) | f09ea9546 → f09ea9546 | — | apps/super-ontology-desktop/ | Drive the P07 open panel with checks and report the attempts it took |
| SO-F-S20-40 | D88 | merged | feat/super-ontology (App 车道 AA 并入) | 0207c9384 → 0207c9384 | — | apps/super-ontology-desktop/ | Add the Ask loop in the main process over a read-only agent token |
| SO-F-S20-41 | D88 | merged | feat/super-ontology (App 车道 AA 并入) | 6d800c276 → 6d800c276 | — | apps/super-ontology-desktop/ | Add the Ask tab under Ontology |
| SO-F-S20-42 | D88 | merged | feat/super-ontology (App 车道 AA 并入) | 9fadc6e2d → 9fadc6e2d | — | apps/super-ontology-desktop/ | Make what is not connected clear in the atlas, Sources and Home |
| SO-F-S20-43 | D88 | merged | feat/super-ontology (App 车道 AA 并入) | 269355e6e → 269355e6e | — | apps/super-ontology-desktop/ | Read a model address kept in a file reference |
| SO-F-S20-44 | D88 | merged | feat/super-ontology (App 车道 AA 并入) | 34f1cdfdb → 34f1cdfdb | — | apps/super-ontology-desktop/ | Let a T1 run use the compose engine and a 2x window |
| SO-F-S20-45 | D88 | merged | feat/super-ontology (App 车道 AA 并入) | abe43ebb8 → abe43ebb8 | — | apps/super-ontology-desktop/ | Fix the Ask request check, number cited sources, and read files honestly |
| SO-F-S20-46 | D88 | merged | feat/super-ontology (App 车道 AA 并入) | 4e47ec6fd → 4e47ec6fd | — | apps/super-ontology-desktop/ | Lock the Ask and Not connected screens with visual baselines |
| SO-F-S20-47 | D88 | merged | feat/super-ontology (App 车道 AA 并入) | 15474193a → 15474193a | — | apps/super-ontology-desktop/ | Check for disagreements before the Ask answer states an owner, date, number or status |
| SO-F-S20-48 | D88 | merged | feat/super-ontology (App 车道 AA 并入) | afcdd5822 → afcdd5822 | — | apps/super-ontology-desktop/ | Refresh the Ask baselines for human anchors, the clear icon and kept-together markers |
| SO-F-S20-50 | D74 | merged | feat/super-ontology (车道 AC 并入) | 9709df077 → 9709df077 | — | — | Judge Not connected across files, not inside one |
| SO-F-S20-51 | D74 | merged | feat/super-ontology (车道 AC 并入) | 45fdf90d6 → 45fdf90d6 | — | — | Extract recordings without waiting for speaker names |
| SO-F-S20-52 | D74 | merged | feat/super-ontology (车道 AC 并入) | eafa99490 → eafa99490 | — | — | Ask for speaker names in Review |
| SO-F-S20-53 | D74 | merged | feat/super-ontology (车道 AC 并入) | 70b4c6a1b → 70b4c6a1b | — | — | Lock the engine image built with provisional speakers |
| SO-F-S20-54 | D74 | merged | feat/super-ontology (车道 AC 并入) | b0a249480 → b0a249480 | — | — | Let the acceptance driver take its Docker lane from the environment |
| SO-F-S20-55 | D74 | merged | feat/super-ontology (车道 AC 并入) | 27ffbd927 → 27ffbd927 | — | — | Keep no_relations as the id of the cross-file reason |
| SO-F-S20-56 | D74 | merged | feat/super-ontology (车道 AC 并入) | f7cbb45df → f7cbb45df | — | — | Hold the statements of an unconfirmed speaker again |
| SO-F-S20-57 | D74 | merged | feat/super-ontology (车道 AC 并入) | 58a454727 → 58a454727 | — | — | Assert the speaker hold in the live and unit tests |
| SO-F-S20-58 | D74 | merged | feat/super-ontology (车道 AC 并入) | c6fe014bd → c6fe014bd | — | — | Show a recording that waits for speaker confirmation as such |
| SO-F-S20-60 | D85 | merged | feat/super-ontology (App 车道 AE 并入) | 2355f1e6d → 2355f1e6d | — | — | Lock the engine image rebuilt for the final build |
| SO-F-S20-61 | D85 | merged | feat/super-ontology (App 车道 AE 并入) | 259065e1b → 259065e1b | — | — | Let the model dialog take file paths for the address and the key |
| SO-F-S20-62 | D85 | merged | feat/super-ontology (App 车道 AE 并入) | efa3ba06d → efa3ba06d | — | — | Press Show more before reading the Not connected list |
| SO-F-S20-63 | D85 | merged | feat/super-ontology (App 车道 AE 并入) | c80f2ad27 → c80f2ad27 | — | — | Allow the model on folders the app creates on this computer |
| SO-B-S13-01 | L8 | merged | feat/super-ontology (车道 C 分支并入) | ac408d725 → c30c203d2 | — | testing/super-ontology/evidence/L8/ | 可选 pgvector 迁移 0144；服务端带 pgvector 时安装向量表 |
| SO-B-S13-02 | L8 | merged | feat/super-ontology (车道 C 分支并入) | 43e47007a → 43e47007a | — | testing/super-ontology/evidence/L8/ | 可选本地向量：BRIEF 其他相关与 resolve 召回 |
| SO-B-S13-03 | L8 | merged | feat/super-ontology (车道 C 分支并入) | cef94147a → 445d2700b | — | testing/super-ontology/evidence/L8/ | 1 万文件规模基准 |
| SO-B-S14-01 | L8 | merged | feat/super-ontology (车道 C 分支并入) | 7c02efdac → 7c02efdac | — | testing/super-ontology/evidence/L8/ | 无 Docker 运行时评审与原型测量（ADR-S14） |

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
