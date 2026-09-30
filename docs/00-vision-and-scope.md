# 00 愿景、定义与范围

> Status: Proposed
> Owner: Open Ontology Engineering
> Last reviewed: 2026-09-30
> 依赖：无。本文是其余所有文档的前提。

## 1. 一句话

**Open Ontology（简称 OO）把企业任何一个陈旧的业务数据库，自动变成一个 Agent 能安全、准确、可追溯地读懂和查询的数据层，并一键接上 Agent harness 进行对话。**

## 2. 要解决的问题

企业内的 Agent 落地，瓶颈通常不在模型，而在数据入口：

| 现状 | 对 Agent 的影响 |
|---|---|
| 业务库是十几年累积的 ERP / TMS / WMS，表名是拼音缩写（`kh`、`ydh`、`sfje`），没有注释 | 模型看得到 schema 但读不懂，写出的 SQL 语法正确、语义错误 |
| 同一业务概念散落在多个库、多张表，状态码是魔法数字，客户名有多个别名 | 模型不知道该 JOIN 哪条路径、`status=3` 是什么、「RPD」是谁 |
| 统计口径只存在于财务和运营人员脑子里 | 「上月收款额」按开票日期还是收款日期，答案差一个量级 |
| 生产库不能被随意查询；单表数千万行 | 让 Agent 直连生产库，一个笛卡尔积就能拖垮业务 |
| 数据权限跟着人走（区域经理只看本区域） | 一个共享的 Agent 账号等于越权 |
| 结果要可审计 | 「这个数字怎么来的」必须能回答 |

市面上的做法大致三类，各有缺口：

1. **直连数据库的 MCP / Text-to-SQL 工具**（如各类 Postgres MCP、Vanna、DB-GPT）：解决了“能查”，但语义层薄弱、直连生产、权限粗糙。
2. **BI 语义层**（dbt MetricFlow、Cube、LookML、Snowflake Semantic View、Databricks Metric View）：语义表达成熟，但需要数据团队手工建模数周到数月，且绑定各自的仓库。
3. **企业本体平台**（Palantir Foundry Ontology）：本体 + 动作 + 权限最完整，但闭源、昂贵、需要驻场实施。

OO 的定位是第 1 类的易用性 + 第 2 类的语义严谨 + 第 3 类的安全边界，并且**开源、可私有化、自动构建**。

## 3. 「Ontology」在本项目中的定义

> 本项目中的 ontology **不是知识图谱**，不引入 RDF/OWL、三元组存储或图数据库。

**Ontology = 一个面向 Agent 的数据库语义与访问层**，由三部分组成：

| 组成 | 内容 | 物理形态 |
|---|---|---|
| **数据面（Data Plane）** | 从源库单向同步来的一比一副本 + 按业务场景构建的派生表/视图 | 独立的 PostgreSQL（Agent DB） |
| **语义面（Semantic Plane）** | 实体、表与字段含义、关系/JOIN 路径、术语与别名、状态码字典、统计口径（指标）、已验证示例查询、数据新鲜度 | Git 中的 YAML + Markdown（Ontology Spec），编译后以只读文件和 API 两种方式提供 |
| **访问面（Access Plane）** | 面向 Agent 的工具协议（MCP / SDK / HTTP / CLI）、按最终用户的权限、SQL 门禁、成本控制、审计 | Gateway + Wrapper + 沙盒 + Agent 入口 |

“Self-build” 指语义面的**初稿由系统自动生成**（catalog 内省、数据画像、关系推断、LLM 起草），人只需要审核和补充关键口径，而不是从零建模。见 [06 Ontology 层](06-ontology-layer.md)。

## 4. 产品形态

### 4.1 目标用户体验（北极星）

```text
$ git clone https://github.com/<org>/open-ontology && cd open-ontology
$ ./oo up                                   # docker compose 一键启动全部组件
# 浏览器打开 http://localhost:8080 → 初始化管理员
# 1. 「添加数据源」：填写 MySQL 只读从库地址 / 上传 CSV、Excel、SQL dump
# 2. 系统做权限体检 → 选择表 → 开始同步（进度条、预计时间）
# 3. 同步完成后自动生成 Ontology 草稿 → 管理员在审核页确认/修改 → 发布 v1
# 4. 「连接 Agent」：
#    - 内置对话页 / 飞书、Slack 机器人（走内置沙盒 + Pi harness）
#    - 或复制 MCP 地址给 Claude Code / Codex / Cursor（带外部客户端 token）
```

从 `oo up` 到第一次对话，目标是**一个小时内**（不含 DBA 开 binlog 的审批时间，不含大表首次同步时间）。

### 4.2 用户角色

| 角色 | 典型人 | 在 OO 里做什么 |
|---|---|---|
| 平台管理员 | 企业 IT / 数据平台工程师 | 部署、登记数据源、配置权限、审核发布 Ontology |
| DBA | 源库负责人 | 按清单开日志、建只读账号；只关心“会不会伤到生产库” |
| 领域专家 | 财务、运营、调度主管 | 审核字段含义、写统计口径、提供评测题目 |
| 最终用户 | 业务人员 | 在群聊/网页里提问，拿到带出处的答案和文件 |
| Agent 开发者 | 企业自己的 AI 团队 | 用 MCP / SDK 把 OO 接进自研 Agent |
| OO 贡献者 | 开源社区 | 写连接器、hook、harness 适配器、评测集 |

## 5. 设计原则

以下原则在所有文档中优先级从高到低，发生冲突时按此排序取舍：

1. **源库零伤害。** 平台对源库只读，读取受限速，优先从只读从库读；系统里不存在写源库业务数据的代码路径。
2. **边界单向、最小权限。** 源库 → 管线 → Agent DB → Gateway → 沙盒，每一跳只能读上游；Agent 手里没有任何可复用的凭证。
3. **正确性可证明。** 同步一致性用对账工具证明；权限用数据库机制兜底，而不是只靠应用层检查；每个答案可追溯到具体 SQL 和数据版本。
4. **诚实优于流畅。** 查不到就说查不到，没权限就说没权限；评测中编造数据直接判错。
5. **自动优先，人工兜底。** 能自动推断的（结构、画像、关系、候选术语）自动生成并标注置信度；关乎口径和权限的必须人工确认后才发布。
6. **一切即代码（Everything as Code）。** 数据源、权限、Ontology、hook 都是 Git 中可 diff、可 review、可回滚的声明式文件；Web 控制台是这些文件的编辑器，而不是另一套真相。
7. **Agent 友好。** 工具少而精、输出为 Agent 设计（截断提示、拒绝原因、改写建议）、上下文渐进披露。
8. **可私有化、无供应商锁定。** 全部组件开源、可离线部署；模型、harness、沙盒实现都可替换。

## 6. 范围

### 6.1 第一版（v0.x，首个客户落地）范围内

- 源：MySQL 8.x、PostgreSQL 13+（含主流云托管版本）；文件上传（CSV / TSV / Excel / Parquet / JSON Lines）。
- 同步：CDC（Debezium）+ 批量增量 + 每日对账；单表数千万行。
- Agent DB：PostgreSQL 16/17 主从。
- Ontology：自动构建草稿、Web 审核、Git 版本化发布、发布前一致性检查。
- 访问：Gateway（只读 SQL、表级/列级/行级权限、代价控制、并发控制、审计）；MCP / Python SDK / HTTP / `oo` CLI。
- Agent 运行：Docker + gVisor 沙盒、出口代理注入凭证、Pi harness、内置网页对话、一个 IM 机器人适配器（飞书）。
- 部署：Docker Compose 一键启动。
- 评测：OO-Bench 公开集（基于合成物流数据）+ 客户私有集的运行框架。

### 6.2 明确不在第一版范围（Non-goals）

| 不做 | 原因 / 替代 |
|---|---|
| 写回源库、任何形式的“动作”（Palantir Actions 类） | 风险面完全不同；以后另起经审批的写回通道（见 [14 路线图](14-roadmap-and-risks.md)） |
| 知识图谱、RDF/OWL、图查询 | 不是本项目的 ontology 定义 |
| 替代数据仓库 / BI | Agent DB 是 Agent 专用副本，不承担企业报表 SLA |
| 实时（亚秒级）一致性 | 目标是秒到分钟级新鲜度，并显式告诉 Agent 数据截至时间 |
| 多租户 SaaS | 第一版以“一个企业一套部署”为单位；内部支持多个项目和多角色 |
| 非结构化文档 RAG（PDF/Word） | 与 ontology 的结构化定位不同；可作为 Knowledge 附件，不做检索引擎 |
| 训练/微调模型 | 通过 Ontology 与评测驱动迭代，模型可替换 |

## 7. 成功标准（第一版）

最终数值需与首个客户确认，下表是初始目标，验证方法见 [13 评测](13-evaluation-and-benchmarks.md)：

| 维度 | 指标 | 初始目标 |
|---|---|---|
| 接入 | 从 `oo up` 到第一个问题得到回答 | ≤ 1 小时（小库） |
| 同步正确性 | 停写后对账差异行 | 0 |
| 同步新鲜度 | CDC 路径 p95 端到端延迟 | ≤ 60 秒 |
| 源库保护 | 平台账号在源库上的写操作 / DDL | 0 |
| 安全 | SQL 攻击用例集、金丝雀凭证扫描、越权用例 | 100% 通过 |
| 回答质量 | 客户题库 pass^3（3 次全对） | ≥ 70%（首版），逐版不下降 |
| 诚实 | 无权限/查不到类题目的正确拒答率 | ≥ 95%，编造率 ≈ 0 |
| 可追溯 | 答案中每个数字可关联到 Gateway 审计中的查询 | 100% |
| 体验 | 简单问题端到端 p50 延迟 | ≤ 30 秒 |
| 成本 | 单次会话模型成本上限 | 可配置，默认硬上限 |

## 8. 与参考设计的关系

`references/` 下的草案给出了七层框架（源库、管线、Agent DB、Gateway、Wrapper、Knowledge 沙盒、Agent 入口）和三道单向边界。本设计保留这些核心判断，并做了以下扩展或修订（理由分别在对应文档里展开）：

| 参考草案 | 本设计 | 见 |
|---|---|---|
| 没有控制面 / 控制台 | 新增 ⓪ 控制面：Web 控制台、Control API、内置 Git、Ontology Builder、评测运行器 | [01](01-architecture.md) |
| Knowledge 全部人工编写，表结构骨架自动生成 | Ontology Spec（结构化 YAML）为唯一真相，自动构建草稿 + 人工审核；Markdown 为编译产物 | [06](06-ontology-layer.md) |
| 平台不在 Agent DB 建任何表 | 允许一个平台保留的派生 schema `_oo`（值索引、画像），可随时重建、不是真相源 | [05](05-agent-db.md) |
| 行级过滤由 Gateway 套在查询外；SQL 检查用 `sqlglot` | 以 PostgreSQL 角色 + RLS 为权威执行；安全判定改用 PG 自身的语法分析器（libpg_query），`sqlglot` 只做方言提示 | [07](07-gateway.md) |
| 只支持数据库源 | 增加文件上传源（DuckDB 解析）与文件托管 | [03](03-sources-and-ingestion.md) |
| ARQ + Redis；Debezium 增量快照经 Redis | ARQ 已进入仅维护模式 → 离散任务改用 Procrastinate（基于控制库），流式装载为常驻 Loader；Redis 8 为 AGPL 等三许可 → 缓冲默认 Valkey；首次快照改为 OO 分块读取直接写 Agent DB（“CDC 优先、快照补空”），不经缓冲、不写源库 | [02](02-technology-stack.md)、[04](04-pipeline.md) |
| 会话结束销毁沙盒 | 完整的会话状态机与“状态释放”清单（token、连接、查询、文件、沙盒、池） | [09](09-sandbox-and-agent-runtime.md) |
| 题库评测 | 六条评测轨道的 OO-Bench + 基于客户数据的迭代闭环 | [13](13-evaluation-and-benchmarks.md) |
