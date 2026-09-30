# 01 总体架构

> Status: Proposed
> Owner: Open Ontology Engineering
> Last reviewed: 2026-09-30
> 依赖：[00 愿景与范围](00-vision-and-scope.md)

## 1. 分层总览

OO 由一个**控制面**和七个**数据/访问层**组成。控制面负责“配置和构建”，七层负责“搬运、存放和访问”。

```text
┌─⓪ 控制面 Control Plane（管理员与领域专家使用）───────────────────────────────┐
│ Web 控制台 · Control API · 内置 Git（配置/Ontology 真相源）· Secret Store        │
│ Ontology Builder（画像/推断/LLM 起草）· 评测运行器 · 控制库（运行状态）          │
└──┬───────────────────┬──────────────────────┬──────────────────────┬──────────┘
   │ oo apply          │ 构建/发布 Ontology    │ 签发角色/策略        │ 编排沙盒配置
   ▼                   ▼                      ▼                      ▼
① 源库（企业现有系统，只读）  ─── binlog/WAL 或 SELECT，限速 ───┐
   MySQL · PostgreSQL · 文件上传（CSV/Excel/Parquet）            │
                                                                ▼
② 管线（单向；Agent/Gateway 不可达）                             
   Extract：Debezium Server（CDC） | 批量 worker | 文件解析 worker
   Buffer：Valkey Streams（仅 CDC 流式变更；首次快照直写 Agent DB）→ 变更批次（Arrow，op + 源位点）
   Transform hooks（脱敏/清洗）   →  Loader（COPY + MERGE，幂等）
   after_load hooks（场景 schema） →  画像/值索引刷新触发
                                                                │ 仅 pipeline_rw 可写
                                                                ▼
③ Agent DB（独立 PostgreSQL：主库 + 流复制只读从库）
   <source>.*  源表一比一副本      <scene>.*  场景表/视图      _oo.*  水位/质量（Agent 不可读）
   oo_dict.*  码表/日历          oo_entity.*  实体/成员/别名（按角色授权）
   角色：pipeline_rw（写） · oo_gateway（登录，只读）→ SET ROLE 到 oo_r_<role>（NOLOGIN） + RLS
                                                                │ 只读从库
                                                                ▼
④ Gateway（唯一数据出口；无状态，多实例）
   认证 → 权限 → SQL 解析/检查/改写 → EXPLAIN 代价 → 执行（角色 + RLS + 超时）→ 截断/导出 → 审计
                                                                │ HTTP API（即 Wrapper 的底座）
                                                                ▼
⑤ Wrapper：HTTP API · Python/TS SDK · oo CLI · MCP Server（远程 Streamable HTTP / 本地 stdio）
                                                                │
          ┌─────────────────────────────────────────────────────┤ 自带 Agent 的客户端直连 ⑤
          │ 经出口代理（按沙盒身份注入会话 token 与模型 Key）      │
          ▼                                                     │
⑥ Knowledge 沙盒（每会话一个；gVisor / E2B 等可插拔）            │
   /knowledge（已发布 Ontology 的只读编译产物）/workspace（中间结果）
   harness（默认 Pi，RPC over stdio）+ oo CLI + Python/DuckDB
   无任何密钥；网络只通出口代理
          ▲ stdio
⑦ Agent 入口（沙盒外，无状态）
   身份校验 → 角色解析 → 取预热沙盒并绑定 → 注入 AGENTS.md → 转发事件流 → 回收与状态释放
          ▲ HTTP / WebSocket / IM 回调
     使用方：内置网页对话 · 飞书/Slack 机器人 · 企业系统
```

## 2. 三道单向边界（不变量）

这三条是系统的**安全不变量**，任何设计变更都不能破坏，并且每条都有自动化测试守护（见 [13 评测](13-evaluation-and-benchmarks.md) 轨道 B/C）。

| # | 边界 | 保证 | 数据库层机制 | 网络层机制 | 测试 |
|---|---|---|---|---|---|
| B1 | 源库 → 管线 | 平台从不写源库 | 登记时 `check_access` 探测账号权限，有写权限默认拒绝；CDC 账号只授复制权限 | 只有管线区可路由到源库 | 权限体检用例 + 源库审计日志比对 |
| B2 | 管线 → Agent DB | 只有管线能写 Agent DB | `pipeline_rw` 仅管线持有；Gateway 登录角色无任何写权限、无 DDL 权限 | Agent DB 主库只对管线区开放；从库只对 Gateway 开放 | 关闭 SQL 检查后的兜底写入用例全部失败 |
| B3 | Agent DB → Gateway → Agent | Agent 只能经 Gateway 读、只读到用户权限内的数据、拿不到凭证 | 每个查询 `SET LOCAL ROLE` 到用户角色 + RLS 策略 | 沙盒只能访问出口代理；代理只转发到 Gateway 与模型服务 | 金丝雀凭证扫描、网络探测、越权矩阵 |

推论：
- **没有写回。** 以后若要“动作/写回”，另起一条独立的、经审批与审计的通道，不复用管线，不给 Gateway 加写接口。
- **Agent 能力 ≤ 最终用户权限。** Agent 永远以“某个最终用户”的身份访问数据，不存在 Agent 超级账号。

## 3. 组件清单与状态归属

明确“谁持有什么状态”是水平扩展和故障恢复的前提。**只有下表标注为有状态的组件需要备份。**

| 组件 | 层 | 有状态？ | 持有的状态 | 持久化位置 | 多实例 |
|---|---|---|---|---|---|
| Web 控制台 | ⓪ | 否 | — | — | 是 |
| Control API | ⓪ | 否 | — | 控制库 / Git | 是 |
| 内置 Git 服务 | ⓪ | **是** | 配置、Ontology Spec、角色策略、hook 代码 | 卷（可切换到企业 GitLab/GitHub） | 单实例 + 备份 |
| Secret Store | ⓪ | **是** | 源库凭证、模型 Key、签名密钥 | 内置加密存储或 OpenBao/Vault/云 KMS | 取决于后端 |
| 控制库（PostgreSQL） | ⓪② | **是** | 运行记录、分块进度、水位镜像、拒绝记录、待分类列、实体 key 登记与管家决定、会话元数据、审计索引、评测结果 | 独立 PG（可与 Agent DB 同集群不同实例） | 主从 |
| Ontology Builder worker | ⓪ | 否 | — | 产物写回 Git | 是 |
| Debezium Server | ② | 位点在外部 | 读取位点、schema 历史 | 控制库（JDBC store；M0 验证，不可用时退回 Valkey） | 每源库 1 个 |
| Streams 缓冲 | ② | **是** | 未消费的变更事件 | Valkey（AOF，`appendfsync always`） | 单主 + 副本 |
| Pipeline worker / Loader | ② | 否 | — | 进度在控制库 | 是（默认按源分片，per_table 按表分片） |
| 对象存储 | ②⑦ | **是** | 上传文件原件、导出结果、会话产出物、会话记录 | S3 兼容 / 本地卷 | 取决于后端 |
| Agent DB 主库 | ③ | **是** | 同步数据 | PG | 1 主 |
| Agent DB 从库 | ③ | 派生 | 主库副本 | PG | N |
| Gateway | ④ | 否 | 限流计数（软状态） | Redis 兼容 | 是 |
| MCP Server | ⑤ | 否 | MCP 会话（可重建） | 内存 / Redis | 是 |
| 出口代理 | ⑥ | 否 | 沙盒身份 → 凭证映射（软状态） | Redis 兼容，TTL 与会话同 | 是 |
| 沙盒 | ⑥ | 临时 | `/workspace` | 沙盒本地，销毁即清除 | 池化 |
| Agent 入口 | ⑦ | 否 | 会话路由（软状态） | 控制库 + Redis | 是 |

## 4. 三条主数据流

### 4.1 接入与同步流（管理员一次操作，系统持续运行）

```text
管理员在控制台“添加数据源” ──▶ Control API 生成 sources/<name>.yaml 的变更（Git 提交/PR）
   ──▶ oo validate（schema 校验、凭证可解析）
   ──▶ oo plan（期望状态 vs 实际状态：新增哪些表、启动哪个 Debezium、需要哪些 DBA 操作）
   ──▶ 权限体检 check_access（只读？复制权限？binlog/WAL 配置？从库延迟？）
   ──▶ oo apply
         ├─ CDC：渲染 Debezium 配置 → 启动容器（只做流式）→ Streams；同时 OO 分块快照直写 Agent DB（CDC 优先、快照补空）
         ├─ 批量：在控制库建分块计划 → worker 并行读 → 变更批次
         └─ 文件：上传到对象存储 → 解析/类型推断 → 变更批次
   ──▶ Loader：影子表/暂存表 COPY → UPSERT → 水位/质量状态同事务更新
   ──▶ 快照完成 → 列分类 → Ontology 发布 → 按角色列级授权（新增列默认隐藏）
   ──▶ 事件：table_ready / schema_changed / freshness_updated → 触发 Ontology Builder
```

### 4.2 Ontology 构建与发布流

```text
table_ready / schema_changed / 定时 / 手动
   ──▶ Builder：catalog 内省 → 数据画像（在 Agent DB 主库上限速执行）
            → 关系推断 → 语义类型 → LLM 起草（描述/术语/候选指标/示例问题）
            → 执行验证示例 SQL → 生成 Ontology Spec 草稿（带置信度与证据）
   ──▶ Git：以分支/PR 形式提交草稿（ontology/draft-<ts>）
   ──▶ 控制台审核页：领域专家逐项确认/修改；未确认的低置信条目不进入发布版本
   ──▶ 发布门禁：spec lint · 与 catalog 一致性检查 · 列分类/权限引用检查 · 指标控制总数/语义差异 · 名称生命周期 · 公开/客户评测回归
   ──▶ 发布：打 tag ontology/vN → 编译 Knowledge Pack（Markdown + JSON）→ 存入对象存储
   ──▶ 新会话使用 vN；进行中的会话继续使用会话开始时的版本
```

### 4.3 问答流（一次用户提问）

```text
用户在飞书群 @机器人 提问
 ──▶ IM 适配器验签 → Agent 入口（携带已签名的最终用户身份）
 ──▶ 入口：解析用户 → 角色绑定 → 取/复用沙盒 → Gateway 签发会话 token（绑定 session_id、sandbox_id、角色、属性）
          → 出口代理登记 sandbox_id → {gateway_token, model_key}
          → 挂载 Knowledge Pack vN 到 /knowledge（只读），写 /workspace/AGENTS.md
          → 启动 harness（Pi RPC over stdio），发送问题
 ──▶ harness 在沙盒内：读 /knowledge/README.md → 找表/口径 → `oo resolve`（实体歧义则澄清）/ `oo query` / `oo search-values`
          → 请求经出口代理（注入 token）→ Gateway 执行门禁 → 从库执行 → 返回（带 query_id、数据截至、水位和质量 warnings）
          → 必要时 `oo export` 大结果 → Python/DuckDB 分析 → 生成图表到 /workspace/outputs
 ──▶ 入口把事件流（思考摘要、工具调用、最终回答）转发给 IM；答案附“所用表/口径/数据截至/时间锚点及日期/质量警告/查询编号”
 ──▶ 会话空闲超时：取回产出物与会话记录 → 吊销 token → 注销代理映射 → 销毁沙盒（见 09 状态释放）
```

## 5. 网络分区与部署拓扑

### 5.1 网络区（逻辑）

```text
 Zone SRC  企业源库网络          ← 仅 Zone PIPE 可达
 Zone PIPE 管线区：Debezium、pipeline worker、Streams、Builder（画像时）
 Zone DATA 数据区：Agent DB 主（仅 PIPE 可达）、Agent DB 从（仅 GW 可达）、控制库
 Zone GW   访问区：Gateway、MCP Server
 Zone SBX  沙盒区：沙盒实例；出站仅到 Zone EGR；入站无
 Zone EGR  出口代理：入站仅来自 SBX；出站仅到 Gateway 与白名单模型端点
 Zone CTL  控制区：控制台、Control API、Git、Secret Store、Agent 入口
 Zone EDGE 入口：反向代理/TLS 终止，暴露控制台、入口 API、MCP（可选）
```

在 Docker Compose 里，每个 Zone 对应一个 Docker network，组件只加入它必需的网络（见 [11 部署](11-deployment-and-operations.md)）。在 Kubernetes 里用 NetworkPolicy 表达同样的规则。

### 5.2 部署形态

| 形态 | 适用 | 说明 |
|---|---|---|
| **All-in-one（Compose）** | PoC、中小库、单机 | 一台 8C/32G 以上的主机；Agent DB 可以单实例（从库可选，关闭时 Gateway 读主库并加严限流） |
| **Standard（Compose 多机 / K8s）** | 首个客户生产 | Agent DB 主从分机；沙盒独立节点池（gVisor）；Gateway ×2 以上 |
| **Scaled（K8s + Helm）** | 大量源库/并发 | 缓冲切换为 Kafka/Redpanda；Agent DB 多从库；沙盒用 agent-sandbox / E2B 自托管 |

## 6. 关键设计决策摘要

完整的选型与取舍见 [02 技术栈](02-technology-stack.md)。

| 决策 | 选择 | 一句话理由 |
|---|---|---|
| Agent 数据落在哪里 | 独立 PostgreSQL 副本，而不是直连源库或虚拟化查询 | 保护源库、统一方言、成熟的权限/超时/RLS |
| 语义层真相源 | Git 中的结构化 Ontology Spec（YAML），Markdown 为编译产物 | 可 diff/review/回滚；可机检；可同时服务文件型 Agent 和 API 型 Agent |
| Agent 如何读语义 | 文件系统（沙盒内）+ `read_knowledge`/`describe_table` API（沙盒外），渐进披露 | 文件+bash 是当前 harness 最擅长的交互方式；API 保证沙盒外可用 |
| Agent 如何取数 | 自由只读 SQL（经 Gateway），而非预定义查询模板 | 长尾问题无法穷举；以门禁 + 数据库权限兜底 |
| 权限执行 | PG 角色 + RLS 为权威；应用层检查用于提前拒绝和可解释错误 | 应用层 SQL 分析不可能完备 |
| 凭证 | 沙盒内零凭证；出口代理按沙盒身份注入 | 提示注入下也拿不到可复用凭证 |
| 同步 | CDC 默认按源事务应用（降级带质量警告），批量兜底，每日对账 | 覆盖删除、低延迟，且可证明一致 |

## 7. 代码仓库结构（规划）

单仓（monorepo），Python 为主，控制台与 TS SDK 为 TypeScript：

```text
open-ontology/
  oo                         # 启动脚本（包装 docker compose 与 oo CLI）
  pyproject.toml             # uv workspace
  packages/
    oo-core/                 # 配置与 Ontology Spec 的 Pydantic 模型、JSON Schema、公共类型、错误码
    oo-connectors/           # 连接器插件：mysql、postgres、file（后续 mssql、oracle、snowflake…）
    oo-pipeline/             # worker、Loader、对账工具、hook 运行时
    oo-builder/              # Ontology Builder：画像、推断、LLM 起草、编译器、发布门禁
    oo-gateway/              # Gateway 服务（FastAPI）+ SQL 检查/改写
    oo-proxy/                # 出口代理
    oo-entry/                # Agent 入口 + harness 适配器（pi、claude-code、codex）+ IM 适配器
    oo-sandbox/              # 沙盒驱动：docker-gvisor、e2b、k8s-agent-sandbox
    oo-control/              # Control API、oo validate/plan/apply 引擎
    oo-sdk/                  # Python SDK + oo CLI
    oo-mcp/                  # MCP Server
    oo-eval/                 # OO-Bench 运行器、评分器、报告
  sdk-ts/                    # TypeScript SDK
  console/                   # Web 控制台（React + TypeScript）
  images/
    sandbox/                 # 沙盒镜像（Pi、oo CLI、Python 数据栈，固定版本）
    debezium/                # 固定版本的 Debezium Server 镜像
  deploy/
    compose/                 # docker-compose.yml 与 profile
    helm/                    # Helm chart（后续）
  bench/
    datagen/                 # 合成物流数据生成器
    suites/                  # 公开评测集（问题、黄金 SQL、攻击用例）
  examples/                  # 示例配置仓库（sources/roles/ontology）
  docs/
```

## 8. 文档间的对应关系

| 层 | 文档 |
|---|---|
| ⓪ 控制面 | [11 部署与运维](11-deployment-and-operations.md)、[12 配置参考](12-configuration-reference.md) |
| ① 源库与文件 | [03 数据源与接入](03-sources-and-ingestion.md) |
| ② 管线 | [04 同步管线](04-pipeline.md) |
| ③ Agent DB | [05 Agent DB](05-agent-db.md) |
| 语义面 | [06 Ontology 层](06-ontology-layer.md) |
| ④ Gateway | [07 Gateway](07-gateway.md) |
| ⑤ Wrapper | [08 Agent 接口](08-agent-interface.md) |
| ⑥⑦ 沙盒与入口 | [09 沙盒与 Agent 运行时](09-sandbox-and-agent-runtime.md) |
| 横切 | [10 安全](10-security.md)、[13 评测](13-evaluation-and-benchmarks.md)、[14 路线图与风险](14-roadmap-and-risks.md) |
