# 02 技术栈与架构决策记录（ADR）

> Status: Proposed（原型跑通后逐条改为 Accepted）
> Owner: Open Ontology Engineering
> Last reviewed: 2026-10-01
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
| 核心语言 | Rust（全部第一方服务、CLI、连接器、Builder、评测器），TypeScript（控制台、TS SDK） | — | 保留 Debezium JVM 与 Pi Node 上游运行时；不把协议接入误当成重写上游 |
| 包管理 / 工具链 | Cargo workspace、Cargo.lock、rust-toolchain.toml、rustfmt、Clippy | MIT/Apache | 精确工具链 + 锁文件；安装用户无需本机 Rust 编译器 |
| Web 框架 | Tokio + Axum + Tower / tower-http；reqwest 出站 HTTP，rustls TLS | MIT/Apache/ISC（按 crate） | HTTP 超时中间件不负责数据库取消，显式清理见 [17](17-rust-implementation.md) |
| 配置 / 语义真相源 | Git 中的 YAML / SQL / Markdown；Serde + Schemars + serde-saphyr + jsonschema | MIT/Apache（按 crate） | 禁止配置标签执行、外部 include 与环境插值；Git 写入通过固定 argv，禁止 shell 拼接 |
| API 规格 | utoipa（OpenAPI 3.1）+ 契约测试 | MIT/Apache | Axum 不自动生成 OpenAPI；不能只靠 Rust 编译证明 API 与 schema 一致 |
| CDC | Debezium Server 3.x（每源库一个容器，JVM） | Apache-2.0 | Kafka Connect 需已有 Kafka；其他连接器的许可见 §5；不自研 Rust binlog/WAL 解码 |
| 变更缓冲 | Valkey Streams（AOF）；Rust redis crate；Kafka 时用 rdkafka | BSD-3 / BSD-3 / MIT（含原生依赖另审计） | redis 是客户端 crate 名，不代表默认部署 Redis 服务；位点、确认、裁剪规则不变 |
| Debezium 位点与 schema 历史 | 控制库 JDBC store；不可用时使用独立持久 Valkey 存储并修订部署计划 | — | Rust 读取事件与管理容器，不直接改写 Debezium offset 格式；缓冲丢失须检测位点缺口 |
| 流式装载 | Tokio 常驻 Loader；tokio-postgres COPY / UPSERT | MIT/Apache | 默认按源分片，per_table 显式降级；Arrow 需要编码为 PG COPY，不能直接发送 IPC 当 COPY |
| 批量任务与定时 | Rust oo-jobs + 控制库 PostgreSQL；SQLx 事务入队、SKIP LOCKED 短事务领取、租约与重试 | 项目 Apache-2.0 | 自研范围限于离散任务；持久工作流复杂时评估 Temporal，见 ADR-015 |
| 数据库驱动 | SQLx：控制库、源 MySQL/PG/SQLite、迁移；tokio-postgres + deadpool-postgres：Gateway 与 Loader | MIT/Apache | 两类池按职责分开；Gateway 需要明确的 portal 和 CancelToken，不能随意换驱动 |
| 读取 / 交换格式 | Apache Arrow Rust（arrow-array/schema/ipc、parquet 同版本）；SQLx 流式行读取转批次 | Apache-2.0（子组件许可按锁文件） | ADBC / ConnectorX 为可选 Rust 优化，不能假定已覆盖所有源、全部类型或零拷贝 |
| 文件解析 | duckdb Rust 绑定（CSV/JSON 嗅探）、calamine（xlsx/xls）、parquet；chardetng + encoding_rs（文本编码） | MIT/Apache/BSD（按依赖） | 独立受限解析进程；DuckDB 为 C++ 原生依赖；Excel 不是无限内存流式接口 |
| Agent DB | PostgreSQL 17（主 + 流复制从）；pg_trgm、pg_stat_statements | PostgreSQL | 角色、列级 GRANT、RLS、只读事务为权威；大版本须与解析器配套 |
| 控制库 | PostgreSQL（独立实例） | PostgreSQL | 任务、会话释放进度、审计与实体 key 都是必须备份的运行状态 |
| SQL 安全判定 | Rust pg_query，直接使用 libpg_query | MIT + BSD-3 + PostgreSQL（各部分） | 不经 Python/PyO3；C FFI 与内嵌 PG 大版本都进入发布审计 |
| 方言提示 / 源 SQL 证据 | Rust sqlparser | Apache-2.0 | 只用于提示与候选提取，不参与 Gateway 放行，不自动执行翻译结果 |
| 权限执行 | PG 角色 + 列级 GRANT + RLS（security_invoker 视图） | — | 不用 Rust 内存过滤替代数据库权限；日历受限例外见 [05](05-agent-db.md) |
| 对象存储 | Rust object_store：默认 LocalFileSystem；S3 兼容后端 | MIT/Apache | 限制路径/前缀、分片、校验与清理；S3 兼容程度通过后端契约测试确认 |
| Secret | Rust 信封加密实现使用审核过的 AEAD / KMS SDK；zeroize / secrecy 管理持有值 | 按实际依赖审计 | 主密钥仅密钥文件/KMS，禁止明文配置；不自行设计密码算法 |
| 沙盒编排 | Docker + gVisor；Rust bollard 经受限 Docker API；K8s 后续用 kube | Apache-2.0（运行时/客户端分别审计） | 编排器持有宿主权限，只在控制区；沙盒中无 Docker socket |
| 出口代理 | Axum / Hyper + reqwest 固定上游反向代理 | MIT/Apache | 禁止任意 CONNECT、关闭自动重定向，严格 URL/头过滤；SSE 有背压 |
| 默认 harness | Pi（Node，RPC over stdio）；Rust 适配器 | MIT | 适配 Claude Code / Codex 等外部进程，不把其语言算作 OO 核心语言 |
| MCP | 官方 Rust rmcp SDK；stdio 与 Streamable HTTP | Apache-2.0 | 按锁定 SDK 实际支持版本协商；OAuth、受众校验、token exchange 由 OO 实现，不假设 SDK 自动完成 |
| SDK / CLI / 本地分析 | Rust oo-sdk（reqwest）、oo-cli（clap）、oo-analysis（Arrow / 图表）；DuckDB CLI | MIT/Apache（按 crate） | TS SDK 保留；本地分析工具也不携带数据库/模型凭证 |
| 可观测 | tracing + tracing-subscriber + tracing-opentelemetry / opentelemetry-otlp；Prometheus | MIT/Apache（按 crate） | exporter 与 SDK 锁定兼容组合；正文/密钥不进入 span；Grafana 仅外部可选 |
| 评测 / 压测 | Rust oo-eval、固定种子生成器；Goose（HTTP）+ pgbench；Criterion（微基准） | Apache-2.0 / PostgreSQL / MIT 或 Apache | Inspect AI / promptfoo 是导出目标，不成为平台运行依赖 |
| 对账 | Rust 哈希分桶，规范化 Arrow 值 | 项目 Apache-2.0 | 使用相同类型策略；不复用不维护的实现 |
| 控制台 / 部署 | React + TypeScript + Vite + shadcn/ui；Docker Compose，后续 Helm | MIT / 按组件 | Rust 运行镜像包含原生库/CA；不能宣称所有二进制均可放 scratch |

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

### ADR-003 批量任务使用控制库队列（Superseded by ADR-015）

- **历史背景：** 修改前设计选择 Python Procrastinate，把任务与状态放入控制库、流式 Loader 单独部署。
- **取代：** Rust 迁移保留“离散任务与流式消费分开”及“控制库事务内入队”原则；现行实现选择与恢复契约以 ADR-015 为准，不再引入旧运行时。

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

- **决定：** 放行/拒绝判定用 Rust `pg_query`（libpg_query）；Rust `sqlparser` 只用于方言提示。实现约束补充见 ADR-014。
- **理由：** 按 PG 大版本配套源生解析器，减少语法差异；原始 AST 不能判断函数重载、运算符或 cast 的实际执行权限，仍需目录校验与数据库兜底。
- **后果：** 直接使用宽松许可 Rust 绑定；内嵌 C/PG 源码也要审计；升级 Agent DB 大版本必须升级解析器并通过攻击/差分测试。

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

### ADR-014 第一方实现改为 Rust，边界保持协议化

- **背景：** 需要在持续 CDC、大结果流式处理和多会话回收下控制资源，统一后端工程栈。
- **决定：** Cargo workspace；Tokio + Axum；Serde / Schemars；Rust SDK/CLI/MCP。SQL 安全判定直接调用 pg_query，方言证据用 sqlparser；Arrow Rust 负责统一批次。TypeScript 前端/SDK、Debezium JVM、Pi Node 通过现有协议保留。
- **后果：** Rust 所有权有助于管理资源，但不能替代网络隔离、数据库权限、显式取消与持久回收；C/C++ FFI 需要原生构建与进程隔离。接口与依赖图见 [17](17-rust-implementation.md)。
- **切换条件：** 个别源没有成熟 Rust 驱动时，新增审核过的外部连接器，而不是向 Gateway 引入其他语言或直连源库。

### ADR-015 Rust 离散任务采用 oo-jobs（取代 ADR-003）

- **决定：** 控制库 jobs schema 的持久任务表，SQLx 实现短事务 SKIP LOCKED 领取、带 fencing generation 的租约、心跳、到期重领、幂等重试、去重与死信；定时器只负责插入任务，数据库保存调度进度。
- **后果：** 不新增 broker；领取后必须立即提交，不在长任务期间持行锁。任务执行至少一次，跨 Agent DB/控制库不能承诺恰好一次或同事务提交；详细恢复规则见 [17 §3](17-rust-implementation.md)。这是一项需要 M0 验证的自研组件，不是已有第三方队列的改名。
- **切换条件：** 持久编排、补偿或队列吞吐超出上述有限功能时，评估 Temporal 等独立服务；先验证其 Rust 客户端与许可证，再新增 ADR。

### ADR-016 插件以显式注册或隔离进程接入

- **决定：** 内置 trait 实现编译进 crate；第三方程序经审核清单启动，用版本化 JSON + Arrow IPC；不加载不稳定的 Rust 动态 ABI，不在生产启动时 cargo install 或编译配置仓库代码。自定义 on_batch hook 无网络、无数据库/Secret 权限。
- **后果：** hook 源码可为 Rust，但部署消费的是已审计的不可变制品；CPU/内存/时限由隔离进程监督，而非给同进程闭包加 timeout。
- **切换条件：** 未来若引入 Wasmtime/WASI，需要另行验证 fuel、内存、IPC 与能力授权；首版不依赖 WASM。

## 4. 缓冲层切换到 Kafka 的条件（沿用参考草案）

满足任一即切换到 Kafka / Redpanda（Debezium Server 同时支持两种 sink，Loader 实现两种消费者）：

- 客户已有 Kafka（Confluent、MSK 等）；
- 源库达到几十个，用 Kafka Connect 统一管理更省事；
- 变更流需要给 Agent DB 以外的系统消费；
- 需要保留多天变更随时重放；
- 变更量持续很大，Valkey 内存扛不住。

## 5. 许可证审计（默认安装）

以下是选型层的许可证边界，**不是已完成所有传递依赖审计的声明**。发布前对 Cargo.lock、JS 锁文件、镜像、原生源码与离线扩展逐一核对 LICENSE/NOTICE，生成 SPDX SBOM；选双许可中允许的一项，不能只按 crate 顶层字段放行。

| 组件 | 许可证 / 处理 |
|---|---|
| OO 第一方 crates、oo-jobs | Apache-2.0 |
| Tokio、Axum、Serde、Schemars、calamine、serde-saphyr | MIT（具体版本以锁文件 LICENSE 为准） |
| SQLx、tokio-postgres、deadpool-postgres、reqwest、object_store | MIT 或 Apache-2.0 |
| pg_query / libpg_query / 内嵌 PG 源码 | 分别 MIT / BSD-3-Clause / PostgreSQL；全部保留 NOTICE |
| Arrow / Parquet、sqlparser、rmcp、Goose、bollard | Apache-2.0 为主；子组件/传递依赖仍需审计；**rmcp 不是旧 Python SDK 的 MIT 许可** |
| DuckDB 与 duckdb-rs | MIT；随镜像链接 C++ 源码，扩展单独审计 |
| encoding_rs / chardetng、rustls、加密与 TLS provider | 多许可组合；按实际启用 feature、原生库及传递依赖审计 |
| Debezium、gVisor | Apache-2.0 |
| Valkey / PostgreSQL / Pi | BSD-3-Clause / PostgreSQL / MIT |
| Grafana、Loki、Langfuse 等观测服务 | 外部可选组件，分别核对 OSS/企业目录许可，不打进 OO 核心镜像 |
| Redis 8、MinIO、Vault、Arize Phoenix | 不默认分发；用户自选外部服务时核对其版本与许可 |

## 6. 版本基线（候选，待 M0 锁定）

下面记录 2026-10-01 的候选系列与兼容约束，不是已编译成功的依赖清单。真正可发布版本必须写入 Cargo.lock / rust-toolchain.toml / 镜像 digest，完成 [17 §7](17-rust-implementation.md) 的验证。新发布未满 7 天的依赖不进入默认制品。

| 组件 | 候选 / 约束 |
|---|---|
| Rust | edition 2024，workspace resolver 3；首轮 MSRV 至少 1.94.0（SQLx 0.9 要求）；固定 stable patch，CI 同时测试 MSRV 与发布工具链 |
| Tokio / Axum / SQLx | 1.x / 0.8.x / 0.9.x；不把 Cargo 默认 feature 当安全配置 |
| tokio-postgres / deadpool-postgres | 0.7.x / 0.14.x；portal、TLS、取消与连接回收联合测试 |
| pg_query | 6.2 系列为候选；必须检查 vendored libpg_query 的 PG_VERSION_NUM，Agent DB 17 仅配套 PG17 parser；不同版本未验证不得部署 |
| Arrow / Parquet | 同一 workspace 锁定同一版本系列；DuckDB 对 Rust Arrow 的依赖可能不同，跨版本经 IPC 而非强转内存对象 |
| DuckDB | 锁定 duckdb-rs 对应的 DuckDB engine patch、扩展与 CLI；不能把 crate 版本号当成 engine 版本号 |
| rmcp | 锁定已发布至少 7 天且通过契约测试的版本；3.5.0 于 2026-09-28 发布，调研日尚不满足入库规则 |
| PostgreSQL / Debezium / Valkey | Agent DB 默认 17.x（16/18 需各自配套 parser）；Debezium 3.5.x 与 JDBC store 待原型核实；Valkey 8.x；镜像锁 patch + digest |
| gVisor / Pi | 固定 release / patch 与镜像 digest；运行时协议和平台能力见 [09](09-sandbox-and-agent-runtime.md) |
| MCP 规范 | 目标 2026-07-28；按 rmcp 实际支持列表协商，上一版客户端兼容性必须有测试证据，不自行宣称自动兼容 |

版本变更是发布事件：必须通过 [13](13-evaluation-and-benchmarks.md) 回归门禁，并写入会话与评测记录。出处与检索记录见 [16 §7](16-references.md)。
