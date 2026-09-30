# 02 技术栈与架构决策记录（ADR）

> Status: Proposed（原型跑通后逐条改为 Accepted）
> Owner: Open Ontology Engineering
> Last reviewed: 2026-09-30
> 依赖：[01 架构](01-architecture.md)。事实与出处见 [16 参考资料](16-references.md)。

## 1. 选型原则

1. **许可证先行：** OO 本身以 Apache-2.0 发布。默认安装的每个依赖必须是 OSI 认可的宽松或弱 copyleft 许可证；AGPL/BSL/ELv2/SSPL 组件只能作为“用户自选的外部服务”，不能打进默认镜像。
2. **少一个有状态组件就少一类故障：** 能复用 PostgreSQL 的就不引入新存储。
3. **维护状态是硬指标：** 进入维护模式或已归档的项目不作为核心依赖。
4. **每个选择都要有切换条件：** 写明“什么情况下换成什么”。
5. **固定版本：** 所有镜像与依赖锁定到 minor/patch；升级走评测回归（[13](13-evaluation-and-benchmarks.md)）。

## 2. 总表

| 领域 | 选择 | 许可证 | 主要备选与不选原因 |
|---|---|---|---|
| 核心语言 | Python 3.12（服务与 CLI），TypeScript（控制台、TS SDK） | — | Go：数据库驱动、Arrow、SQL 解析生态不如 Python；控制台用 TS 是前端事实标准 |
| 包管理 | uv（workspace） | MIT/Apache | Poetry：多包 workspace 支持弱 |
| Web 框架 | FastAPI + Pydantic v2 + uvicorn | MIT/BSD | — |
| 配置 / 语义真相源 | Git 中的 YAML / SQL / Markdown，内置 Git 服务或接企业 Git | — | 存数据库：失去 diff、review、回滚 |
| CDC | Debezium Server 3.x（每源库一个容器） | Apache-2.0 | Kafka Connect：需先有 Kafka；Flink CDC：需运维 Flink；Airbyte：连接器已转 ELv2；PeerDB：AGPL；Estuary：BSL；Redpanda Connect：CDC 输入为企业许可证；Sequin / Supabase ETL：仅 PG 源 |
| 变更缓冲 | **Valkey** Streams（Redis 协议兼容，AOF） | BSD-3 | Redis 8：AGPL/RSAL/SSPL 三许可，不进默认镜像（用户可自行替换，协议兼容）；Kafka/Redpanda：见 §4 切换条件 |
| Debezium 位点与 schema 历史 | 控制库（JDBC offset/history store）；不可用时退回 Valkey | — | 与缓冲同放一个 Valkey：缓冲实例丢失会同时丢位点（原型中验证 JDBC store 的可用性） |
| 流式装载 | 自研常驻 Loader 服务（asyncio，默认按源分片；per_table 按表分片） | — | 用通用任务队列承载常驻消费：语义不匹配 |
| 批量任务与定时 | **Procrastinate**（基于控制库 PostgreSQL 的任务队列） | MIT | ARQ：官方已进入仅维护模式；Celery：asyncio 支持弱；Temporal/Hatchet：多一套服务，第一版不需要持久化工作流；Dramatiq：LGPL 且需 broker |
| 读取 / 交换格式 | Apache Arrow（pyarrow）；ADBC / ConnectorX 做批量读取 | Apache-2.0/MIT | 行式 Python 对象：内存与速度差一个数量级 |
| 文件解析 | DuckDB（CSV/Parquet/JSON/xlsx 读取与类型嗅探）+ charset-normalizer | MIT | pandas 直接读：大文件内存不可控 |
| Agent DB | PostgreSQL 17（主 + 流复制从），扩展 `pg_trgm`、`pg_stat_statements` | PostgreSQL | DuckDB：并发访问与权限能力不足；ClickHouse：RLS/超时/角色体系不如 PG 成熟，且 Agent 的 SQL 方言成本高 |
| 控制库 | PostgreSQL（独立实例或同集群独立库） | PostgreSQL | — |
| SQL 安全判定 | **pglast**（libpg_query，即 PG 自身的语法分析器） | GPL-3.0-or-later ⚠ / 见 §5 | sqlglot：自研解析器，与 PG 实际语法存在差异，只用于方言提示 |
| 方言提示 | sqlglot | MIT | — |
| 权限执行 | PG 角色 + 列级 GRANT + RLS（`security_invoker` 视图） | — | 纯应用层改写：不可能完备；OPA 部分求值 → SQL：多一套策略语言，第一版角色模型足够简单 |
| 对象存储 | 抽象层（obstore / fsspec）：默认本地卷；可接任意 S3 兼容（客户 S3、SeaweedFS、Garage 等） | — | MinIO 社区版：2025 年起进入维护模式、2026 年起仅源码分发，不作为默认 |
| Secret | 内置加密存储（信封加密，主密钥来自环境/KMS）；可插拔 OpenBao / Vault / 云 KMS / Infisical | — | 明文 env：禁止 |
| 沙盒 | 默认 Docker + gVisor（runsc）；可插拔 E2B（自托管）、kubernetes-sigs/agent-sandbox（Kata/gVisor） | Apache-2.0 | 本机 OS 级沙盒（bubblewrap/seatbelt）：适合开发者本机，不适合多用户服务端 |
| 出口代理 | 自研“固定上游的反向代理”（ASGI），不做通用 CONNECT 代理 | — | Envoy + 凭证注入过滤器：可行，作为 K8s 形态的备选；mitmproxy：需要 TLS 中间人 |
| 默认 harness | Pi（RPC 模式，stdio JSON 协议） | MIT | 自研 agent loop：维护成本高；可替换为 Claude Agent SDK / Codex app-server / OpenCode（统一适配器接口） |
| MCP | 官方 Python SDK，跟随 2026-07-28 规范（无状态核心，Streamable HTTP） | MIT | — |
| 可观测 | OpenTelemetry（trace/metric/log）+ Prometheus + Grafana；可选 Langfuse（自托管） | Apache/AGPL(Grafana, 外部) | Arize Phoenix：Elastic-2.0，不进默认安装 |
| 评测 | 自研 `oo-eval`（任务/求解器/评分器模型借鉴 Inspect AI），可导出到 Inspect / promptfoo | MIT | — |
| 压测 | Locust（MIT）、pgbench | MIT/PG | k6：AGPL，可作为用户自选 |
| 对账 | 自研哈希分桶对账（算法同 data-diff/reladiff） | — | datafold data-diff 已归档；reladiff 可作为参考实现与交叉校验 |
| 控制台 | React + TypeScript + Vite + shadcn/ui | MIT | — |
| 部署 | Docker Compose（默认）；Helm chart（M3） | — | 只提供 SaaS：企业数据通常不能出内网 |

## 3. ADR 列表

每条 ADR 的格式：背景 → 决定 → 后果 → 切换条件。

### ADR-001 Agent 数据落在独立 PostgreSQL 副本中

- **背景：** 备选有三：Agent 直连源库（或其从库）、查询虚拟化（Trino 等联邦查询）、独立副本。
- **决定：** 独立副本。
- **后果：** 源库零查询负载（除同步读取外）；统一为 PG 方言，Agent 只需学一种 SQL；可以使用 PG 的角色/RLS/超时作为权限与资源的权威边界；代价是存储翻倍、有秒到分钟级延迟，需要对账保证一致。
- **切换条件：** 若某数据源体量远超单个 PG 能承载（数十 TB 级分析负载），为该源评估“同步到列式引擎 + 同样的 Gateway 门禁”的形态（见 [14](14-roadmap-and-risks.md)）。

### ADR-002 CDC 用 Debezium Server，不上 Kafka

- **背景：** 第一版每个客户只有几个到十几个源库、一个消费方。
- **决定：** Debezium Server（每源库一个容器）→ Valkey Streams。默认每个源库一个 stream（用 topic 路由 SMT 把该源的全部表与事务元数据汇入，保持提交顺序，支撑 ADR-011）；`per_table` 模式下每张表一个 stream（Debezium Redis sink 的默认映射）。
- **后果：** 不需要运维 Kafka；Valkey 内存成为背压点：Debezium Redis sink 在 `used_memory ≥ memory.threshold.percentage`（默认 85%）时停止消费，**连接器整体暂停而不丢数据**；Loader 跟不上时源库日志会积压，需监控（PG 复制槽 WAL、MySQL binlog 保留）。
- **切换条件（任一）：** 客户已有 Kafka；源库达到几十个；变更流需要多个消费方；需要保留多天可重放；Valkey 内存持续告警。缓冲层做成接口，Loader 同时实现 Streams 与 Kafka 两种消费者，切换只改配置。

### ADR-003 批量任务用 Procrastinate，流式装载用常驻服务

- **背景：** 参考草案选择 ARQ。调研确认 ARQ 已进入仅维护模式。
- **决定：** 定时增量、每日对账、重做快照、Ontology 构建、评测运行等**离散任务**用 Procrastinate（任务与状态都在控制库，支持事务内入队、`SKIP LOCKED` 领取、定时任务）；**常驻的流式消费**是独立的 Loader 服务，不走任务队列。
- **后果：** 去掉“Redis 同时承担队列与缓冲”的耦合；任务状态与分块进度、水位在同一个库，可以在一个事务里“推进水位 + 标记任务完成”。
- **切换条件：** 需要跨天的长工作流、复杂补偿逻辑时，评估 Temporal 或 DBOS。

### ADR-004 语义真相源是 Git 中的结构化 Ontology Spec

- **决定：** OOS（YAML）为真相源；Markdown/JSON 为编译产物；generated/curated 两层 overlay。详见 [06](06-ontology-layer.md)。
- **后果：** 可 diff、可 review、可回滚、可机检；控制台必须把 Git 操作包装得对非工程人员友好（审核即提交）。
- **切换条件：** 无。可以增加导出格式（OSI/Apache Ossie、dbt、Cube）。

### ADR-005 Agent 以“文件 + 少量工具”读语义，以自由只读 SQL 取数

- **背景：** 两种主流形态：语义层编译 SQL（Cube、Cortex Analyst、Metric View）与 LLM 直接写 SQL（Genie、Vercel d0）。Vercel 的公开实验表明，把语义层写成文件、让 Agent 用 bash 浏览，比二十多个专用工具更快、更省 token、成功率更高。
- **决定：** 沙盒内 Knowledge 为文件；数据访问工具只有 `query`、`export`、`resolve`、`search-values`、`describe`、`freshness` 等少数几个；Agent 写自由 SQL，由 Gateway 门禁 + 数据库权限兜底。
- **后果：** 长尾问题可答；风险转移到门禁与评测上。
- **切换条件：** 若评测显示特定高价值指标的口径错误率居高不下，为这些指标提供 `compile_metric` 工具（M3）。

### ADR-006 权限以 PG 角色 + RLS 为权威执行点

- **决定：** 每个 OO 角色对应一个 `oo_r_<role>` NOLOGIN 角色；Gateway 在每个查询的事务中 `SET LOCAL ROLE` 与 `SET LOCAL oo.claims`；行级过滤写成 RLS 策略；业务视图使用 `security_invoker`，日历仅按 [05 §6.3](05-agent-db.md) 使用受限所有者权限例外。详见 [05 §4](05-agent-db.md)。
- **后果：** 应用层检查被绕过时权限仍然成立；角色策略变化需要 DDL（由 `oo apply` 生成，经复制到从库）。
- **切换条件：** 若出现高基数、强动态的授权需求（例如按项目成员关系的 ReBAC），评估 OpenFGA/SpiceDB + 由 Gateway 下发 claims 数组的方式，RLS 仍为执行点。

### ADR-007 SQL 判定使用 PostgreSQL 真实语法分析器

- **决定：** 放行/拒绝判定用 pglast（libpg_query）；sqlglot 只用于方言提示。
- **理由：** 安全检查的解析器与执行数据库的解析器必须一致，否则“解析器认为是 SELECT、数据库实际执行了别的东西”的差异就是绕过点。
- **后果：** 许可证需确认（见 §5）；pglast 与 PG 大版本绑定，升级 PG 时一起升级。

### ADR-008 沙盒内零凭证，出口代理按沙盒身份注入

- **决定：** 沙盒只能访问出口代理；代理是“固定上游”的反向代理：`/gateway/*` → Gateway（注入会话 token），`/llm/<provider>/*` → 模型端点（注入模型 Key）；不支持任意域名。
- **后果：** 即便被提示注入诱导，沙盒里也找不到可带走的凭证，也无法访问任意外网（切断“致命三要素”中的外传通道）。harness 必须支持自定义模型 base URL（Pi、Claude Code、Codex 均支持）。
- **切换条件：** 无。

### ADR-009 默认 harness 为 Pi，通过统一适配器可替换

- **决定：** Pi 以 RPC 模式在沙盒内运行，入口通过 stdio JSON 驱动；适配器接口抽象“启动、发送消息、事件流、中止、导出会话记录”。
- **后果：** 入口不需要在沙盒里开端口；Pi 于 2026 年转由 Earendil 维护（MIT），需要锁定版本并跟踪上游。
- **切换条件：** 评测显示其他 harness 在同等模型下明显更优时，切换默认值；适配器保证切换不影响沙盒、代理与 Gateway。

### ADR-010 默认对象存储为本地卷 + S3 兼容抽象

- **决定：** 不内置 MinIO；默认使用本地卷，生产推荐企业已有的 S3 兼容存储。
- **后果：** all-in-one 部署少一个组件；多机部署必须配置共享的 S3 兼容存储（沙盒产出物、导出文件、Knowledge Pack 需要跨节点访问）。

### ADR-011 CDC 默认按源事务原子应用

- **背景：** 按表独立提交会让 Agent 在 JOIN 时看到源库中从未存在过的“半个事务”，而业务动作（开票、发货）几乎总是跨表的。
- **决定：** `consistency: transactional` 为默认：每源一个有序 stream，Loader 按 Debezium 事务元数据把完整源事务放进同一个 PG 事务；超大事务分段写入并带质量警告；`per_table` 为显式降级选项。详见 [04 §5.2](04-pipeline.md)。
- **后果：** 单源吞吐受单个 Loader 分片约束；需要验证 topic 路由 SMT 与事务元数据在 Debezium Server + Redis sink 下的行为（M0 P11）。
- **切换条件：** 单源峰值变更超过单分片能力且业务可接受跨表短暂不一致时，该源改为 `per_table`；或切换到 Kafka 后按事务 ID 分区。

### ADR-012 只做列级授权，新列默认不可见

- **背景：** PG 的表级 `SELECT` 授权自动覆盖之后新增的列；源库新加一列敏感字段，就会未经分类地对 Agent 可见。
- **决定：** 对 `oo_r_*` 一律逐列授权；只授权已完成分类（`pii`、`semantic_type`）并已发布的列；Gateway 展开 `SELECT *` 为可见列。详见 [05 I-DB-5](05-agent-db.md)、[06 §4.10](06-ontology-layer.md)。
- **后果：** 首次接入需要完成列分类（规则高置信的非个人信息列可自动接受，其余批量确认）；`SELECT *` 成为 Gateway 唯一的 SQL 改写。
- **切换条件：** 无。

### ADR-013 平台维护的、Agent 可读的 `oo_dict` 与 `oo_entity`

- **背景：** 码值含义、时间锚点、实体合并如果只存在于 Markdown 中，Agent 只能把它们抄进 SQL：抄错、抄漏、新码被算进口径、“上季度”被算错、同一客户被当成三个。
- **决定：** 把已发布的码表（带类别、状态与有效期）、业务日历锚点、实体解析结果物化进 Agent DB 的 `oo_dict` 与 `oo_entity`，由平台维护、可重建；认证指标与 Agent 的 SQL 直接 JOIN 它们。详见 [05 §6.3–6.4](05-agent-db.md)、[06 §3.6、§3.9、§3.10](06-ontology-layer.md)。
- **后果：** 放宽了“平台不在 Agent DB 建 Agent 可见对象”的原则；实体键登记与管家决定成为需要备份的运营状态（控制库）。
- **切换条件：** 无。

## 4. 缓冲层切换到 Kafka 的条件（沿用参考草案）

满足任一即切换到 Kafka / Redpanda（Debezium Server 同时支持两种 sink，Loader 实现两种消费者）：

- 客户已有 Kafka（Confluent、MSK 等）；
- 源库达到几十个，用 Kafka Connect 统一管理更省事；
- 变更流需要给 Agent DB 以外的系统消费；
- 需要保留多天变更随时重放；
- 变更量持续很大，Valkey 内存扛不住。

## 5. 许可证审计（默认安装）

| 组件 | 许可证 | 处理 |
|---|---|---|
| Debezium | Apache-2.0 | ✓ |
| Valkey | BSD-3-Clause | ✓ |
| PostgreSQL | PostgreSQL License | ✓ |
| Procrastinate | MIT | ✓（发布前核对 LICENSE 文件） |
| DuckDB | MIT | ✓ |
| pyarrow / ADBC | Apache-2.0 | ✓ |
| pglast | **需核对**：pglast 本身为 GPL-3.0-or-later（libpg_query 为 BSD-3） | ⚠ 若确认为 GPL，首选方案是基于 libpg_query 的宽松许可绑定（例如 Rust `pg_query` crate（MIT）+ PyO3 薄封装，作为 `oo-sqlparse` 子包维护）；次选是把判定逻辑做成独立进程/服务。M0 结束前定案 |
| sqlglot | MIT | ✓ |
| gVisor | Apache-2.0 | ✓ |
| Pi | MIT | ✓ |
| MCP Python SDK | MIT | ✓ |
| Locust | MIT | ✓ |
| Grafana | AGPL-3.0 | 作为可选外部组件（compose profile `observability`），不打包进 OO 镜像 |
| Langfuse | MIT（`ee/` 除外） | 可选 |
| Arize Phoenix | Elastic-2.0 | 不默认提供 |
| MinIO | AGPL-3.0，维护模式 | 不默认提供 |
| Redis 8 | AGPL/RSAL/SSPL | 不默认提供，协议兼容可自选 |

## 6. 版本基线（初始）

| 组件 | 版本 |
|---|---|
| PostgreSQL（Agent DB / 控制库） | 17.x（18.x 在原型中验证） |
| Debezium Server | 3.5.x |
| Valkey | 8.x |
| Python | 3.12 |
| DuckDB | 1.x 最新稳定版 |
| gVisor | 最新稳定 release（按日期版本） |
| Pi | 锁定具体版本号，随镜像发布 |
| MCP 规范 | 2026-07-28（兼容 2025-11-25 客户端） |

版本变更是发布事件：必须通过 [13](13-evaluation-and-benchmarks.md) 中的回归门禁，并写入会话记录与评测记录。
