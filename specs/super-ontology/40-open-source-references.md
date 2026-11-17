# Super Ontology 开源借鉴

> 核查日期 2026-11-16。star 数取当天 GitHub 页面读数；标“未核”的没有打开一手页面确认。许可证与商用限制在引入依赖前要再核一次。
> 结论已经落进 [00 决策记录](00-overview-and-decisions.md)，本文记录依据，方便以后复盘或换方案。

## 1. 总判断

没有一个开源项目同时做到下面四件事：

1. 证据精确到原文位置（页 + bbox、单元格区间）；
2. 每条事实带置信度，并经人工门禁才发布；
3. 按任务编译一份有 token 预算、逐行带出处的简报；
4. 按共享范围做物理隔离，范围外的内容连名字都不泄露。

各家只做到其中一两件。Super Ontology 的做法是：每一段借最好的那一家，把四件事串成一条流水线，这本身就是差异点。

## 2. 文档解析

| 项目 | 许可 | 结论 | 借什么 |
|---|---|---|---|
| [Docling](https://github.com/docling-project/docling)（70.7k） | MIT（模型各自有许可） | **默认解析器**（D13） | 每个元素带 `prov`（page_no、bbox、charspan）；TableFormer 的单元格网格（row/col span、column_header）；docling-serve 提供 HTTP 服务。注意：DOCX 和 MD 的 prov 为空（[#1012](https://github.com/docling-project/docling/discussions/1012)），这两种格式的锚点由我们自己按标题路径加行号生成 |
| [PaddleOCR / PP-StructureV3](https://github.com/PaddlePaddle/PaddleOCR)（92.8k） | Apache-2.0 | **M2 的 OCR** | 中文最强；表格给出单元格坐标；印章识别；跨页表格合并 |
| [calamine](https://github.com/tafia/calamine)（2.5k） | MIT | **Excel 走 Rust 内** | 合并单元格 API、公式读取。合并区里只有左上角有值，需要自己展开填充 |
| [SpreadsheetLLM / SheetCompressor](https://arxiv.org/pdf/2407.09025v1) | 论文 | 表头与表格区域检测的算法 | 按值、样式、合并区的变化找结构锚点，IoU 去重叠，相同值合成区间。社区实现：[kingkillery](https://github.com/kingkillery/Spreadsheet_LLM_Encoder) |
| [Xberg（原 Kreuzberg）](https://github.com/kreuzberg-dev/kreuzberg)（9.6k） | MIT | 备选：Rust 原生快路径与 MIME 识别 | 项目刚改名并重开 v1 版本线，质量没有公开基准，先观察 |
| [MinerU](https://github.com/opendatalab/MinerU)（83.8k） | Apache-2.0 + 附加条款 | **默认关，待法务评估** | 中文、公式、表格质量最好。附加条款：对外提供在线服务须显著标注使用了 MinerU；集团月活超过 1 亿或月收入超过 2000 万美元须另签许可 |
| [Marker](https://github.com/datalab-to/marker) | 代码 Apache-2.0，权重为修改版 OpenRAIL-M | **不用** | 融资或营收超过 500 万美元须付费 |
| [Unstructured](https://github.com/Unstructured-IO/unstructured) | Apache-2.0 | **不用** | 开源版表格单元格准确率 0.426（付费版 0.866）；默认开遥测 |
| [MarkItDown](https://github.com/microsoft/markitdown) | MIT | 只做预览或降级 | 没有版面和坐标信息 |
| [olmOCR](https://github.com/allenai/olmocr) | Apache-2.0 | 不用 | 需要 GPU，只输出 Markdown，没有 bbox |

## 3. 本体抽取与时间

| 项目 | 借什么 | 不借什么 |
|---|---|---|
| [Graphiti](https://github.com/getzep/graphiti)（32.9k，Apache-2.0） | 双时间：`valid_at`/`invalid_at` 加 `created_at`/`expired_at`；取代时旧边只失效不删除；`identity_properties` 做确定性合并；每条事实都能回溯到 episode → **D17** | 冲突完全由 LLM 判定、没有人工环节；查询默认不过滤失效边（我们默认过滤）；未来时间的 invalid_at 会被立即标为过期（[#2012](https://github.com/getzep/graphiti/issues/2012)） |
| [TrustGraph](https://github.com/trustgraph-ai/trustgraph)（2.9k，Apache-2.0） | 按本体切片约束抽取，违反 domain/range 的三元组直接拒绝 → **D15**；PROV-O 血缘（文档 → 页 → chunk → 边）；Context Core 与 `.tgx` 把三元组、出处、原文一起打包 → Context Pack 的先例 | 整套平台过重 |
| [KAG](https://github.com/OpenSPG/KAG)（9.2k） | 同一类型上可以混用 schema 约束和开放抽取 → **D16 的“A 带一点 B”**；知识与 chunk 互相索引 | 最后一个版本是 2025-06，活跃度在下降 |
| [LightRAG](https://github.com/HKUDS/LightRAG)（40.7k，MIT） | 删除文档时复用 LLM 缓存重建受影响的实体 → **D22、D23**；用 `MAX_SOURCE_IDS_PER_RELATION` 限制每条关系挂的证据数 | 实体消歧弱（基本靠同名合并） |
| [Neo4j LLM Graph Builder](https://github.com/neo4j-labs/llm-graph-builder)（5.3k） | 可配置允许的节点和关系类型；按相似度阈值找重复节点，进合并候选 → `entity_merge` 队列 | 依赖 Neo4j |
| [Cognee](https://github.com/topoteretes/cognee)（33.3k） | 可导入外部本体；把会话知识沉淀进图谱（类似我们的 Suggest） | — |
| GraphRAG、mem0 | — | GraphRAG 已进入维护模式，成本高、增量支持弱；mem0 只做 ADD、没有人工审核，和我们的门禁理念相反 |
| [RAGFlow](https://github.com/infiniflow/ragflow)（94.2k） | DeepDoc 的 chunk 可视化与人工干预 → 详情栏的“解析结果”标签页 | 是平台，不是库 |

## 4. 上下文打包

| 项目 | 借什么 | 落在哪 |
|---|---|---|
| [Agent Skills 规范](https://agentskills.io/specification) | 三级渐进披露：元数据约 100 token → 正文不超过 5000 token → 附带资源按需加载，引用只深一层 | BRIEF.md → INDEX.md → 子文件（§6.3） |
| [Repomix](https://github.com/yamadashy/repomix)（29.3k，MIT） | `--token-budget` 超预算返回非零退出码；按 glob 分级降精度（全文 / compress / 只留目录）；`--token-count-tree`；用 Secretlint 剔除敏感内容 | D25 的降精度与 manifest 中的 token 数；`oo context mount` 的退出码 |
| [Context7](https://github.com/upstash/context7)（64.9k） | 把版本写进稳定 ID | SHARE.json 中钉住的 drive 版本与本体版本 vN |
| [Basic Memory](https://github.com/basicmachines-co/basic-memory)（4.6k，AGPL-3.0，只参考不拷代码） | Markdown 是唯一数据源；MCP 工具标注 read-only 或 destructive | M2 的 MCP `suggest` 工具标注 |
| llms.txt、AGENTS.md | 约定文件名入口 | Context Pack 根目录里给 Agent 的一行提示 |

## 5. 写回与审核

| 来源 | 借什么 |
|---|---|
| [Tiptap Tracked Changes](https://tiptap.dev/docs/ai/ai-toolkit/agents/tracked-changes)（Pro，闭源） | AI 写入的是修订而不是直接改文档；修订带 reason；可以按作者或范围接受；接受或拒绝的结果回传给 AI |
| [carsonhung/track-changes](https://github.com/carsonhung/track-changes)（Apache-2.0）、[sungkhum/tiptap-track-changes](https://github.com/sungkhum/tiptap-track-changes) | 开源的修订交互实现，L5 的修订模式视图可以参考 |
| [Cursor Memories 的兴衰](https://archcore.ai/blog/cursor-memories-removed/) | **反面教材。** 1.2 版做了逐条弹窗审批，2.1 版整个删掉，残留的记忆还继续进入上下文 → D24：批量队列 + 按来源整体接受；删除必须彻底 |
| GitHub PR 审核 | 结构化的本体变更用 diff 展示；文档正文改动用修订模式。两种并存 |

## 6. S3 同步

- 事件通知是“至少一次投递、不保证顺序”（[AWS](https://docs.aws.amazon.com/AmazonS3/latest/userguide/notification-how-to-event-types-and-destinations.html)）。同一个 key 用 `sequencer` 判断先后，但 sequencer 不能按字符串比较；去重键是 key + versionId + operation + sequencer（[AWS 博客](https://aws.amazon.com/blogs/storage/manage-event-ordering-and-duplicate-events-with-amazon-s3-event-notifications/)）。没开版本控制时，并发写可能只发一条事件 → **D10：必须定期对账**。
- multipart 上传和加密对象的 ETag 不是 MD5；阿里云 OSS、腾讯云 COS 提供 crc64 头（[OSS](https://help.aliyun.com/en/oss/developer-reference/completemultipartupload)、[COS](https://intl.cloud.tencent.com/document/product/436/34078)） → **D10：内容主键用本地计算的 BLAKE3**。
- MinIO 社区版 2025-12 进入维护模式，2026 年已归档（[LWN](https://lwn.net/Articles/1077739/)） → **D9：测试用 RustFS**（[rustfs/rustfs](https://github.com/rustfs/rustfs)，Apache-2.0，2026-09 发布 1.0）。
- Rust 客户端：[OpenDAL](https://github.com/apache/opendal) 原生支持 s3/oss/cos，有 capability 检查和重试 layer → **D8 主路径**。`object_store`（OO 已在用，0.14.2，只开了 fs）不支持 OSS/COS 原生协议。需要 ListObjectVersions、checksum 时，直接调 aws-sdk-s3。

## 7. 网盘交互

| 来源 | 借什么 | 注意 |
|---|---|---|
| [Cloudreve](https://github.com/cloudreve/cloudreve)（29.0k，GPL-3.0） | 最接近百度网盘：并行断点续传、带有效期的分享、多存储后端 | GPL，只参考交互，不拷代码 |
| [OpenList](https://github.com/OpenListTeam/OpenList)（25.7k，AGPL-3.0） | 目录内的 README 直接渲染在列表上方 → Super Drive 文件夹页顶部的 BRIEF 预览卡 | AGPL，只参考 |
| Google Drive 的三档角色 | 查看者 / 评论者 / 编辑者 → viewer / suggester / editor | — |
| Nextcloud 全文索引 | **反面教材。** 索引状态只能在管理员命令行里看到 → 我们把处理状态做成文件列表里的一列，失败可以在界面上直接重试 | — |
| 百度网盘、Google Drive | 右下角的上传与处理队列面板、面包屑、列表与网格切换、右键菜单、多选批量操作 | 凭产品经验，未逐项抓一手文档 |
