# 11 部署、控制面与运维

> Status: Proposed
> Owner: Open Ontology Engineering
> Last reviewed: 2026-10-01
> 依赖：[01 架构](01-architecture.md)、[02 技术栈](02-technology-stack.md)

## 1. 一键启动

```bash
git clone https://github.com/<org>/open-ontology && cd open-ontology
./oo up                 # = oo init（首次）+ docker compose --profile standard up -d + 健康检查
```

`./oo up` 首次运行时执行 `oo init`：

1. 预检：Docker ≥ 24、Compose v2、CPU/内存/磁盘、`runsc` 是否已注册为 Docker runtime（未安装时给出一条安装命令；开发模式可 `--insecure-sandbox` 降级并持续警告）；
2. 生成部署密钥：Secret Store 主密钥（写入 `./data/secrets/master.key`，权限 0600，提示管理员离线备份）、内置 CA、各服务 mTLS 证书、Agent DB 各角色随机密码；
3. 初始化内置 Git 仓库（`config.git`），写入示例目录结构（`sources/`、`roles.yaml`、`ontology/`、`hooks/`、`agent.yaml`）；
4. 初始化控制库 schema（SQLx 编译内嵌 SQL 迁移），Agent DB 基础对象（`_oo` schema、`oo_owner` 等角色、函数撤权）；
5. 启动全部服务并等待健康检查通过；打印控制台地址与一次性管理员初始化链接。

离线/内网环境：`oo bundle export` 在联网机器上把全部镜像（固定 digest）、目标平台的 Rust 二进制、DuckDB 扩展、CA 与 SBOM/校验清单 打成离线包，`oo bundle import` 在内网导入。

### 1.1 Rust 制品与平台约束

正式目标宿主为 Linux（x86_64/aarch64，需分别验收）；macOS/Windows 的 Docker Desktop 不等于已具备 gVisor，生产预检不得静默降级。使用固定 builder 工具链的多阶段镜像，运行层保留必要原生库/CA/时区/字体；用户一键启动无需 Rust 编译器。离线扩展/插件按 digest 打包，运行时禁止下载/在线编译。见 [17 §7](17-rust-implementation.md)。

## 2. Compose 服务与网络

```yaml
# deploy/compose/docker-compose.yml（示意）
networks:
  edge: {}                       # 反向代理对外
  ctl: { internal: true }        # 控制面
  pipe: { internal: true }       # 管线 ↔ 缓冲 ↔ Agent DB 主
  src: {}                        # 管线 → 源库（需能路由到企业网络）
  data_ro: { internal: true }    # Gateway ↔ Agent DB 从
  gw: { internal: true }         # 代理/入口/MCP ↔ Gateway
  sandbox: { internal: true }    # 沙盒 ↔ 出口代理（仅此二者）
  llm: {}                        # 出口代理 → 模型端点

services:
  edge-proxy:     { networks: [edge, ctl, gw] }            # Caddy：TLS、控制台、入口 API、MCP
  console:        { networks: [ctl] }
  control-api:    { networks: [ctl, pipe] }                # oo apply 需要对 Agent DB 主库执行 DDL
  git:            { networks: [ctl] }                      # 内置 Git（可替换为企业 GitLab）
  control-db:     { networks: [ctl, pipe] }            # 各服务使用不同的最小权限账号
  valkey-buffer:  { networks: [pipe] }                     # appendfsync always
  valkey-state:   { networks: [gw, ctl] }                  # 限流、token 缓存、代理注册表
  loader:         { networks: [pipe] }
  worker:         { networks: [pipe, src] }                # Rust oo-jobs：快照、增量、对账、文件
  builder:        { networks: [pipe, llm] }                # Ontology Builder（调用 LLM 起草）
  agentdb-primary: { networks: [pipe] }
  agentdb-replica: { networks: [pipe, data_ro] }            # 复制流来自 pipe，查询来自 data_ro
  gateway:        { networks: [gw, data_ro] }
  audit-collector: { networks: [gw, ctl] }                # 仅 audit_ingest 写控制库审计，不提供业务写 API
  mcp:            { networks: [gw, edge] }
  agent-entry:    { networks: [ctl, gw, edge] }            # 需要 Docker API（通过 socket-proxy，最小权限）
  docker-socket-proxy: { networks: [ctl] }                 # 只开放 container create/start/stop/attach/rm
  egress-proxy:   { networks: [sandbox, gw, llm] }
  # debezium-<source>: 由 oo apply 动态创建，networks: [pipe, src]
  # sandbox-<id>:     由 agent-entry 动态创建，runtime: runsc，networks: [sandbox]
```

要点：

- 沙盒只在 `sandbox` 网络上，该网络只有出口代理；`internal: true` 保证没有到宿主外的路由。
- standard profile 的 Gateway 只在 `gw` 与 `data_ro` 上，碰不到主库、缓冲和源库网络。minimal profile 在主库上新增专用 `data_ro` 查询接口与只读角色，显式记录没有物理只读从库的降级；权限与只读事务仍需完整验证。
- Gateway 每实例的审计 WAL 使用独立持久卷；滚动替换前确认重放，不能随临时容器删除。collector 入库按事件去重，磁盘满时 Gateway 停止数据准入。
- 入口不直接挂载 Docker socket，而是经过只放行必要 API 的 socket 代理。
- Compose profile：`minimal`（单机、无从库、无观测）、`standard`（默认）、`observability`（Prometheus、Grafana、Loki、OTel Collector，Grafana 为 AGPL 外部组件）、`dev`（热加载、调试端口，不做生产校验）。

### 2.1 主机规格（初始建议）

| 形态 | 规格 | 说明 |
|---|---|---|
| PoC（minimal） | 8 vCPU / 32 GB / 500 GB SSD | 单机；源数据 ≤ 5,000 万行总量；并发会话 ≤ 5 |
| 首个客户（standard，三机） | A：控制面 + 管线（8C/32G）；B：Agent DB 主从（16C/64G ×2 或同机双实例）；C：沙盒节点（16C/64G，gVisor） | 并发会话 ≈ 20 |
| 扩展（K8s） | 按组件独立扩缩 | 见 §8 |

最终规格以性能测试（5,000 万行单表）结果为准。

## 3. 控制面

### 3.1 控制台功能

| 模块 | 功能 |
|---|---|
| 数据源 | 添加向导（连接 → 体检 → 选表 → DBA 清单 → 计划 → 应用）；每表状态、进度、新鲜度、错误；暂停/恢复/重做快照；文件上传与解析预览 |
| Ontology | 审核队列（逐表、专家问答卡片、差异、列分类、未解释码值、实体合并/否定/拆分）；实体/关系图；指标编辑与认证；示例管理；业务日历结账锚点维护（记录操作者、旧值、新值与生效时间）；发布（控制总数、语义差异与名称生命周期门禁）与回滚 |
| 权限 | 角色编辑（表/列/行级过滤）、绑定规则、“以某用户身份预览可见数据”、外部客户端 token 管理 |
| Agent | harness/模型/预算配置；`AGENTS.md` 追加内容；使用方（网页、飞书、Slack）接入；会话列表与会话回放（含关联的 SQL 审计） |
| 评测 | 题库管理、运行评测、结果对比、错误归因 |
| 审计 | Gateway 调用检索、拒绝原因统计、导出 |
| 运维 | 组件健康、延迟与积压、告警、紧急停止 |

### 3.2 控制台自身的权限

| 角色 | 能做什么 |
|---|---|
| `owner` | 全部，包括 Secret 与紧急停止 |
| `admin` | 数据源、权限、Agent 配置、发布 |
| `steward`（数据管理员） | Ontology 编辑与审核、指标认证（需为该指标 owner）、评测 |
| `reviewer`（领域专家） | 专家问答卡片、描述审核 |
| `auditor` | 只读审计与会话回放 |

控制台登录走企业 OIDC；所有写操作生成 Git 提交或控制库审计记录。“以某用户身份预览”本身会写入审计。

### 3.3 `oo` 管理命令（与 Agent 用的 `oo` 同一个 CLI，不同子命令组）

```text
oo validate                 # 校验配置仓库（schema、引用、权限、hook SQL）
oo plan                     # 期望状态 vs 实际状态的差异
oo apply [--auto-approve]   # 执行：启动/停止 Debezium、建表、快照、GRANT/RLS、Knowledge 编译
oo status                   # 组件与表状态
oo source check <name>      # 重新体检
oo table resnapshot <t>
oo reconcile <t> [--full]
oo ontology build|lint|diff|publish|rollback
oo eval run <suite> [--compare <run_id>]
oo session list|kill <id>|kill --user <u>
oo token issue|revoke
oo backup|restore
oo emergency-stop
```

## 4. 升级

- 版本号遵循 SemVer；每个版本附升级说明（数据库迁移、配置变更、破坏性变更）。
- 顺序：备份控制库 → 升级控制面（SQLx 编译内嵌 SQL 迁移，校验 checksum、单执行者锁、先扩展后收缩，向前兼容）→ 滚动升级 Gateway/入口/MCP/代理 → 升级 Loader/worker（transactional 按源租约迁移，per_table 按表租约迁移）→ 升级 Debezium（逐个源库，位点在控制库）→ 最后升级沙盒镜像（新会话生效）。
- Agent DB 的 PG 小版本：先从库后主库；大版本：新建实例 + 管线重建或 `pg_upgrade`，按变更窗口进行。
- 任何影响 Agent 行为的升级（harness、模型、沙盒镜像、Knowledge 编译器）必须先通过评测回归门禁（[13 §8](13-evaluation-and-benchmarks.md)）。

## 5. 备份与恢复

| 对象 | 方式 | RPO / RTO（初始目标） |
|---|---|---|
| 控制库 | pgBackRest 全量 + WAL 归档；包含实体 key 登记表、管家合并/否定/拆分决定与结账锚点变更记录 | 5 分钟 / 1 小时 |
| 内置 Git | 每日打包 + 推送到企业 Git 镜像（可选） | 24 小时 / 30 分钟 |
| Secret Store | 加密导出；主密钥离线保管 | — |
| Agent DB | 生产默认 pgBackRest（PoC 可关闭；数据可从源库重建） | 15 分钟 / 1 小时（有备份时） |
| 对象存储 | 依赖后端的版本化/复制 | 按后端 |
| Valkey 缓冲 | AOF 持久化并监控位点缺口；丢失时隐藏受影响表并从安全位点恢复/重做快照，再全量对账 | — |

每季度一次恢复演练：从备份恢复控制库与 Agent DB，运行全量对账，结果存为 Evidence。实体派生表可重建，但须先恢复 key 登记表与管家决定，不能重新分配已发出的 key；重建后验证源行唯一成员关系和实体汇总控制总数。

## 6. 可观测性与 SLO

### 6.1 信号

- **Trace：** 一次提问的完整链路：入口 → harness 轮次 → 每次 `oo` 调用 → 代理 → Gateway → SQL 执行；`session_id`、`turn_id`、`query_id` 作为属性。OpenTelemetry GenAI 语义约定中的模型调用属性（模型名、token 用量）在代理处记录；该约定中的 Agent/工具部分仍处于开发状态，字段名以锁定版本为准。
- **Metrics：** 见各层文档（管线 [04 §11](04-pipeline.md)、Gateway [07 §10](07-gateway.md)）。
- **Logs：** 结构化 JSON；凭证模式脱敏；不记录查询结果与模型正文。

### 6.2 SLO（初始）

| SLO | 目标 |
|---|---|
| Gateway 可用性（非 5xx 比例） | 99.9% / 30 天 |
| Gateway 自身开销 p99 | < 30 ms |
| CDC 端到端延迟 p95 | < 60 s |
| 首条消息获得沙盒 p95 | < 1 s（池命中） |
| 简单问题端到端 p50 | < 30 s |
| 会话关闭 → token 吊销 | < 5 s（100%） |
| 会话关闭 → 沙盒销毁 | < 60 s（99.9%） |

## 7. 常见运维手册（Runbook 索引）

| 场景 | 要点 |
|---|---|
| PG 源库复制槽积压告警 | 确认 Debezium/Loader 状态；Loader 慢则扩容或检查 Agent DB 主库；必要时暂停该源 CDC 并在 DBA 确认后删除复制槽（之后需重做快照） |
| MySQL binlog 即将被清理 | 同上；联系 DBA 临时延长保留 |
| Valkey 内存高 | 找出积压的 stream；检查是否有表处于暂停但未写入隔离区；扩内存或切 Kafka（ADR-002） |
| 表被暂停（结构变化） | 查看 schema diff；确认映射后选择回放隔离区或重做快照；重新构建该表的 Ontology |
| 对账持续有差异 | 查看差异行样本；判断是类型映射问题（修连接器）还是丢事件（查 Valkey 持久化与位点）；块修复或重做快照 |
| 新列待分类 / 未解释码值积压 | 列先确认 pii 与 semantic_type，码值由业务 owner 批准；发布并 apply 后才扩大授权或业务类别，期间保留隐藏/质量警告 |
| 转换拒绝 / 指标控制总数失败 | 查掩码拒绝记录与独立对账结果；修策略后重放或修复，复验后清除状态；指标未通过时降级并告知 |
| 实体校验失败 / 错误合并 | 管家执行否定或拆分决定；保留旧 key 历史；重建成员与别名并复验，不允许自动猜测 |
| 超大事务分段中断 | 保留最后完整水位及不一致标记，从源事务起点幂等重放；完成前不得清除警告 |
| Gateway 大量 `COST_TOO_HIGH` | 查看被拒 SQL 的共性；补充索引建议、场景表或 Knowledge 提示 |
| 从库热备冲突频繁 | 检查换表时间与长查询；调整 `max_standby_streaming_delay` 或错峰换表 |
| 沙盒启动失败 | 检查 runsc、宿主机资源、镜像拉取；预热池降级为按需创建 |
| 模型服务故障 | 代理返回可识别错误；入口向用户说明；可配置备用模型端点 |
| 疑似凭证泄露 | 紧急停止 → 轮换相关凭证 → 审计取证（[10 §13](10-security.md)） |

## 8. Kubernetes（M3）

- Helm chart：每个无状态组件一个 Deployment + HPA；Agent DB 推荐使用成熟的 PG operator（例如 CloudNativePG）或企业托管 PG；Valkey 用 StatefulSet 或托管服务。
- 沙盒：kubernetes-sigs/agent-sandbox 的 `Sandbox` 与 `WarmPool`；RuntimeClass 为 gVisor 或 Kata；专用节点池（taint）。
- 网络：NetworkPolicy 表达 §2 中的全部网络规则（默认拒绝）；沙盒命名空间只允许到出口代理的出站。
- Debezium：每个源库一个 Deployment（replicas=1），由控制面通过 K8s API 管理。

## 9. 对象存储前缀与保留期

| 前缀 | 内容 | 默认保留 |
|---|---|---|
| `uploads/` | 上传文件原件 | 180 天 |
| `knowledge/` | 编译后的 Knowledge Pack | 永久（小，内容寻址） |
| `sessions/<id>/outputs/` | 会话产出物 | 30 天 |
| `sessions/<id>/transcript.jsonl` | 会话记录 | 180 天 |
| `sessions/<id>/workspace.tar.enc` | `/workspace` 快照（可选） | 24 小时 |
| `quarantine/` | 暂停表的原始变更事件 | 直到处理完成 + 7 天 |
| `audit-anchors/` | 审计哈希链根（WORM） | 按合规要求 |
| `eval/` | 评测运行产物 | 1 年 |
