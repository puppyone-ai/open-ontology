# 17 Rust 实现契约与迁移验收

> Status: Proposed — 工程契约，尚无产品实现代码
> Owner: Open Ontology Engineering
> Last reviewed: 2026-10-01
> 依赖：[02 技术栈](02-technology-stack.md)、[04 管线](04-pipeline.md)、[07 Gateway](07-gateway.md)、[09 运行时](09-sandbox-and-agent-runtime.md)、[10 安全](10-security.md)

## 1. 迁移范围与 crate 关系

本次迁移改变实现语言与工程机制，保留 ontology 的定义：Agent 专用 PostgreSQL 数据副本、经审核的语义资产、按最终用户权限访问。源库零写入、单向边界、列分类、质量警告、状态释放与客户私有评测仍是硬约束。

| crate / 制品 | 依赖方向与职责 | 凭证与访问边界 |
|---|---|---|
| oo-core | Serde 类型、Schemars schema、错误码、配置/OOS 静态校验；被其他 crate 使用 | 不依赖具体数据库、沙盒或模型客户端；无密钥持有 |
| oo-connectors | 依赖 oo-core；SQLx 只读读取、类型转换与 Arrow 组批 | 仅管线部署可解析源库 Secret |
| oo-jobs | 依赖 oo-core + SQLx；控制库任务/租约 | 独立任务角色；不是 Loader 消费缓冲 |
| oo-pipeline | oo-connectors + oo-jobs + Arrow + tokio-postgres；变更应用、COPY、对账、hook 调度 | 源库只读；持 pipeline_rw；与 Gateway 不可达 |
| oo-builder | oo-core + oo-jobs；元数据、画像、LLM 草稿与编译 | 画像只读账号；源逻辑由管线采集，不持源库密码；专用 Builder 模型凭证 |
| oo-control | oo-core + oo-jobs；Git、计划、apply、发布；独立 audit-collector binary 使用专用 audit_ingest 角色 | 管理权限只在控制区；DDL 经授权 apply 执行 |
| oo-gateway | oo-core + pg_query + tokio-postgres + Arrow；认证、门禁、RLS 查询、审计 | 不链接连接器或管线写入模块；只持只读角色；审计走受限内部接收接口 |
| oo-sdk / oo-cli / oo-mcp | oo-core HTTP 契约；SDK 用 reqwest；CLI 用 clap；MCP 用 rmcp | 不连接数据库、不执行平台管理 DDL；CLI 管理子命令调用 Control API，须单独认证 |
| oo-proxy | oo-core + Axum/Hyper + reqwest/rustls | 仅固定上游，持会话 token 与模型 Key；不向沙盒返回它们 |
| oo-sandbox / oo-entry | oo-core；bollard / 后续 kube；harness 通过 stdio JSON 对接 | 编排权限只在控制区；会话运行状态在控制库 |
| oo-eval / bench/datagen | oo-core 契约、SDK、评分器；固定种子 Rust 生成器 | 客户私有集仅在客户部署；评测 token 只连冻结副本 |
| oo-analysis | Rust Arrow/Parquet、图表工具，按 CLI 合约预编译 | 沙盒中只分析授权导出文件，不持数据连接凭证 |
| console / sdk-ts | TypeScript；共享版本化 JSON Schema / OpenAPI 3.1 | 不因生成客户端而获得任何管理权限 |

依赖箭头从服务指向公共库，公共库不得反向引用服务。共享代码不等于共享进程：Docker 网络、数据库角色与 Secret 分配仍按组件隔离。Debezium JVM、Pi Node、PostgreSQL、Valkey、DuckDB engine 保持上游制品，通过协议接入。

## 2. Tokio、查询与状态释放

- HTTP 路由由 Axum / Tower 提供。请求体、SQL 长度、并发、等待队列及响应体都必须有上限；大文件上传使用单独的有界分片入口，不套 Gateway 的 1 MB 请求上限。
- Gateway 的每次查询独占一个池连接及一个 REPEATABLE READ READ ONLY 事务；禁止同一连接跨用户并发、pipeline 多个主体查询。角色名仅来自编译权限快照，安全引用标识符；claims 用平台固定参数化 SQL 设置，不拼接 JSON 字面量。
- Agent SQL 必须经 `prepare()` → `bind()` → `query_portal()`，每次取正数的有界页；portal 的 `max_rows=0` 表示取全部，禁止作为默认值。不把 Agent SQL 传给简单协议、管理批处理或 COPY。
- 连接持有者登记 `(session_id, query_id, connection_generation, CancelToken)`。超时/断连/吊销触发独立清理任务，先停止接收与输出，再发送取消，等待执行退出、回滚、重置、检查连接。迟到的取消不得作用于已归还并借给下一个用户的连接；等待取消和清理完成前禁止复用。
- 用 `CancellationToken` 传播停止意图，`JoinSet` / TaskTracker 跟踪任务。取消 future、Tower timeout、`JoinHandle::abort` 均不能证明 PG 后端已停止；Transaction Drop 只作兜底，不能替代已确认的异步 rollback。超过清理时限即销毁连接，数据库 statement_timeout 作第二道界限。
- CPU 密集但可信的批处理可用受 semaphore 限制的 `spawn_blocking`；已经开始的 blocking 任务不能靠 abort 终止。上传文件、原生解析器及第三方 hook 放独立受限进程，超时后杀进程/容器，确认退出再记完成。
- SIGTERM：停止准入 → 取消活跃工作 → 有界等待清理 → 刷写审计/任务状态 → 关闭池。会话释放状态仍持久化到控制库，重启 reaper 从未完成的 S1–S11 继续；Rust RAII 不会销毁外部沙盒或吊销远端 token。

## 3. oo-jobs、Loader 与事务恢复

### 3.1 控制库任务队列

`jobs.tasks` 至少包含 `id/kind/payload_version/payload/config_version/dedupe_key/state/run_at/attempt/max_attempts/lease_owner/lease_until/generation/last_error`，状态为 `queued/running/succeeded/failed/cancelled`；终止失败进入可审计的死信视图。配置版本/制品 digest 固定在任务中，不在重试时换用最新配置。

- 控制库事务内写业务状态和入队；`SELECT … FOR UPDATE SKIP LOCKED` 领取可运行任务，原子更新租约/generation 并立即提交；执行网络、解析和长任务期间不保持领取事务或行锁。
- 心跳、完成、取消、续租均比较 owner 与 generation；租约到期可重领，旧 worker 不再有权提交任务状态。定时任务在 `jobs.schedules` 记录下次触发时间，以 `(schedule_id, scheduled_at)` 唯一键避免多实例重复入队；NOTIFY 仅作唤醒，轮询兜底。
- 指数退避 + jitter、有限重试与目标去重；任务必须可重入、可取消。`jobs.leases` 管理同目标互斥，不能只靠内存锁。
- 心跳/fencing 不会自动撤销旧 worker 的数据库写能力。对 Loader 分片，在目标 Agent DB 同一专属连接持有 advisory lock；丢失该连接必须停止写入，新 owner 获目标锁后才运行。跨库操作以幂等业务键及目标端 generation/检查点避免旧结果提交；不得只在控制库检查后就假设目标写入安全。

### 3.2 跨库与 Streams

Agent DB 数据、`_oo.table_freshness`、`_oo.source_watermark`、质量汇总在目标同一事务提交；控制库的任务状态/水位镜像是之后可重试的独立事务。目标提交后才 XACK；控制库提交失败时重试镜像，不回退目标数据；崩溃后按幂等键/位点重放。不得把两个 PG 实例称为“同一事务”。

`XTRIM MINID` 必须低于所有未确认条目及未完成事务的最早 stream ID。原始事务暂存未被耐久存储/应用前，不能因另一已完成事务而裁掉其事件；内存缓冲、磁盘溢出和租约切换均测试重放。Debezium 读取位点不是 Loader 已应用位点，缓冲丢失后不能简单从较晚位点继续。

源位点之外保留事务内事件顺序：同一事务对同主键多次更新/删除/重建可能有相同提交 LSN，须按事务内序号合成最终值，特别是 TOAST 不可用列，不能只取位点最大的一条。Loader 重放源事务的最终批次仍按提交位点幂等。

## 4. 插件、hook 与不可信文件

- Rust trait 使用 `async-trait` 保证需要的动态分派，方法返回 `Result` 与有界 `Stream + Send`；不把运行时插件做成 `dlopen` 任意 Rust dylib。内置实现通过注册表显式列出支持能力。
- 配置仓库保留 Rust hook 源码以供审核；CI 构建制品，生成 SPDX、签名与 digest；生产仅消费不可变清单，禁止运行时 Cargo 拉取、build.rs 执行或“安装插件”自动编译。第三方 crate 的 build.rs/proc-macro 也只在无生产 Secret 的隔离构建环境执行。
- 外部连接器与纯 hook 是不同能力：连接器只获它所需源的只读访问，on_batch hook 只获 Arrow IPC 输入和输出，网络关闭、不挂 Secret 或宿主路径。运行在 gVisor/等价受限容器；输出仍校验 schema、行数、op/位点与系统列；过滤策略必须显式配置并告知质量影响。
- JSON 控制协议与 Arrow IPC 均带版本、长度上限、批次序号、schema 指纹；错误脱敏、协议不匹配即拒绝。清单允许的 type、entrypoint、网络和挂载均由管理员审核，不接受 Agent 提交清单。
- DuckDB/calamine/Parquet 解析 worker 只挂载当前上传副本与受限临时目录，不持源库或 Agent DB 密钥。SQL dump 恢复容器无外网、无宿主权限、无生产凭证，即使 dump 执行任意 SQL 也仅影响临时实例；恢复完成后管线只读抽取。
- 禁止 DuckDB 运行时安装/自动加载扩展、远程 URI 和任意文件访问；导入工具固定生成 SQL，路径参数安全绑定并限制在当前上传目录。批准的扩展仅在构建阶段离线打包、核验签名与版本。分析沙盒本地 DuckDB 可读 `/workspace`，网络仍由沙盒边界限制。
- Excel/压缩文件限制压缩比、解压字节、sheet/单元格/字符串数、内存、CPU、时长。5 GB 是上传存储上限，不是解析能力保证。导出 Excel 用 Rust 写入库或既定 CLI 制品，不执行来自数据的公式。

## 5. 类型、Arrow 与文件托管

- 每个 `RecordBatch` 最大行数与字节数独立限制，超大单元格由类型策略处理；mpsc、有界 stream、HTTP body、COPY sink 全链路背压，禁止默认 `collect()` 全表到内存。
- 所有 Arrow crates / Parquet 在 workspace 内锁同一系列。DuckDB 内部依赖另一 Arrow 系列时经 Arrow IPC 边界传递并重新验证 schema；不强转不同 Rust 类型或宣称 IPC 全程零拷贝。
- `_oo_pos` 用 checked i128/u128 转换验证 decimal128(38,0) 范围；epoch 不得无限左移。超范围即暂停并报警，不能 wrap。PG 的任意精度 numeric 不保证全部可装进 Decimal128 / 常用 Rust Decimal：超过表示范围时用 Decimal256 或经声明的无损十进制字符串与精度/scale 元数据；禁止经 f64 中转。
- JSON bigint/numeric、位点与精确金额以字符串或类型化值传输；时间保留源时区/无时区语义，DST 歧义必须走 type_policy。NaN/Infinity、负零、字节、数组、jsonb 与 null 使用版本化规范哈希，不对 Arrow 物理缓冲直接哈希（缓冲布局/字典编码不等于逻辑值）。类型双路径测试比对规范值/哈希与 schema。
- SQLx 源类型解码遇到未知、自定义/超范围值时先取原始文本/字节再显式映射，不依赖驱动自动做有损转换。COPY 是 PG wire 格式，Rust Loader 显式编码，所有标识符来自已审核 catalog 并引用，所有值不进入 SQL 拼接。
- object_store 的 key 从平台生成的 upload/session ID 和哈希构造；原文件名仅显示，绝不作为任意本地路径。限制前缀、禁止绝对路径/`..`/符号链接穿越；恢复 tar 也校验条目类型与路径。
- 大对象流式上传/下载、分片限额与 sha256 校验；失败或会话关闭时 abort multipart、清理临时文件，残留由 reaper 对账。S3 的 TLS、签名、加密、ACL、短期鉴权与保留期由明确配置决定，object_store 抽象不会自动赋予这些保证。

## 6. SQL、HTTP 与 Secret 的安全边界

`pg_query` 的 raw AST 不是 SQL 语义分析器：遍历所有 CTE/子查询、限定标识符、函数与运算符/类型 cast；用户可调用的自定义函数、SECURITY DEFINER、扩展、隐式调用均审计。函数白名单不能只比较名字，须验证实际允许的 schema/重载、可信目录和 GRANT，不能允许业务 schema 放同名函数。

解析前限制输入长度；C FFI 解析在受限工作进程中执行并限制时长/内存，解析后限制 AST 节点/深度。星号展开、deparse 后再次 parse 与门禁；保留字面量的 AST 才用于口径差异，fingerprint 不用于证明 SQL 等价。解析器不支持执行库的新语法时 fail closed，不回退到 sqlparser 放行。

出口代理验证固定路径与上游 origin，丢弃沙盒提供的认证、Host、转发及 hop-by-hop 头；禁用自动重定向，模型端点 DNS/IP 变化按白名单策略验证，防 SSRF / DNS rebinding / 密钥跨域发送。Gateway mTLS 客户端身份由真实 TLS 连接取得，不能信任公开客户端自报证书头。

rmcp 只提供协议/transport 能力：OO 负责 Origin 检查、认证、RFC 9728 元数据、资源受众、PKCE、撤销、RFC 8693 token exchange 与限额。MCP session ID 不是身份凭证；stdio 外部 token 的存储与远程无状态服务分别处理，日志只写 stderr。SDK 的沙盒模式禁止读取外部 token，LLM 凭证只在代理，返回 body/错误/redirect 也经过凭证脱敏。

Secret 值使用 secrecy / zeroize 并避免 Debug/Clone；零化只能减少部分内存残留，无法保证库副本、core dump、swap 不泄露。生产禁止 core dump、限制调试权限，TLS 配置使用企业 CA/主机名验证。内置信封加密使用审核的 AEAD、唯一 nonce、带版本的密文与 KMS/密钥文件，禁止自创加密协议。

审计写入必须经受限 collector：Gateway 在 [01](01-architecture.md) 的网络中不能直接写控制库。collector 用控制库 `audit_ingest` 最小权限账号接收，不提供其他业务写接口；Gateway 在对外返回数据前持久记录准入，结束再记录结果/拒绝/取消，collector 失败时本地 fsync WAL（独立持久卷），重放按 `(query_id, phase, sequence)` 去重，准入与结束是独立事件，不能按 query_id 丢掉后续结果。磁盘满或审计无法持久化时停止数据准入，不能靠无界异步队列宣称“不丢”。

## 7. 构建、部署与验收

Rust 服务多阶段构建：builder 固定工具链、Cargo.lock 与原生构建工具；runtime 非 root，带所需动态库、CA、时区、字体与已批准扩展。pg_query 构建涉及 C / bindgen / libclang / protobuf 工具，duckdb 涉及 C++；具体工具需求以锁定版本 build.rs 为准。在 Linux x86_64 与 aarch64、gVisor 上验证；不假设在 macOS 编译成功即可部署 Linux。

离线部署用户只导入已签名镜像/平台制品，不需要 Cargo 网络或编译器；需要离线重构建时额外提供 `cargo vendor` 配置、锁定原生源码与 builder 镜像。在还没有 Cargo.toml 和代码时，下列命令是未来 CI 验收要求，不能声称已执行产品构建。

| 验收 | 要求 / 未来命令 |
|---|---|
| Rust 基础 | `cargo fmt --all -- --check`；`cargo clippy --workspace --all-targets --locked -- -D warnings`；`cargo test --workspace --locked`；审核后的各 feature 组合另测，不盲目启用互斥 all-features |
| 依赖与许可证 | `cargo audit`；`cargo deny check`；`cargo tree -d` 检查 Arrow/TLS 重复版本；原生源码、JS、镜像与 OSV 一起审计 |
| API / Spec | 同一 fixture 经 YAML → Serde → JSON Schema → OpenAPI/TS/Rust SDK；未知字段、重复键、时间、数值精度、错误与导出中断契约一致 |
| 同步 A | CDC/快照/文件类型规范值一致；同主键同事务多事件与 TOAST；目标提交/ACK/控制库完成各处强杀；旧 lease owner 不得继续写 |
| 权限 B | 空参数多语句被 PG 协议拒绝；关闭应用门禁仍不能写库、改 claims 或越权；RLS/列级权限与连接池跨用户不串状态 |
| 隔离 C / 状态释放 | timeout/断连/吊销/SIGTERM/panic 后无残留后端查询、portal、池连接、任务许可、multipart、容器；blocking worker 超时真正退出；reaper 恢复 S1–S11 |
| Ontology D / Agent E | 固定冻结快照、as_of、种子、Rust 生成器/评分器版本；保留 dev/holdout 分界；口径、实体与质量警告不回退 |
| 性能 F | Goose/pgbench/Criterion + Linux/gVisor 实机测试；记录 RSS、批次背压、COPY/IPC 吞吐、查询取消时延、回收时限；不因改为 Rust 就宣布原性能目标已达成 |
| 部署 | 一键 Compose fresh install、升级/备份恢复、离线与两种 CPU 架构；迁移 checksum 与单执行者锁；失败时不启动依赖未完成 schema 的服务 |

本次文档迁移的可验证范围是文档、链接、配置语法与接口片段；M0 的运行/安全/性能验证完成后才将对应 ADR 从 Proposed 改为 Accepted，实现与测试存在后才改为 Implemented。

## 8. 本次文档自查记录（2026-10-01）

修改前已将原始 18 份文档 Git 存档，提交 `c6c5137d63e37b867ca5f6d72e04257383dabc9b`。本次读取全部原文，更新 18 份文档并新增本契约；历史 ADR 的旧语言仅作为已被取代的决策记录保留。

- 检查全部 19 份文档的本地链接、标题锚点、代码围栏、旧实现依赖与原有安全不变量；修正无状态/审计 WAL、释放顺序、数据库支持范围等交叉表述。
- 4 个 JSON 片段通过严格 JSON 解析；19 个 YAML 片段通过实际 `serde-saphyr` 解析（仅语法与重复键检查，不代表业务配置 schema 验收）。
- 4 个 Rust 片段通过 rustfmt 语法解析；3 个 trait 接口在临时 Cargo 工程中使用实际 async-trait、Tokio、Arrow 依赖编译，并验证 `dyn Trait` 动态分派。未定义的业务类型用占位类型补齐，仅证明接口签名成立；SDK 使用示例尚无产品 crate，未作可运行承诺。
- `git diff --check` 通过；调研资料与版本限制记录在 [16 §7](16-references.md)。没有改动客户数据，也没有创建产品 workspace 或宣称完整依赖图已构建。

文档阶段未发现未处理的 Rust 迁移矛盾；仍待验证的产品假设保留在 [14 M0](14-roadmap-and-risks.md) 和本章 §7，必须在实现时取得运行证据。
