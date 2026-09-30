# 07 Gateway：身份、权限、SQL 门禁、成本控制与审计

> Status: Proposed
> Owner: Open Ontology Engineering
> Last reviewed: 2026-09-30
> 依赖：[05 Agent DB](05-agent-db.md)、[06 Ontology](06-ontology-layer.md)
> 被依赖：[08 Agent 接口](08-agent-interface.md)、[09 沙盒](09-sandbox-and-agent-runtime.md)、[10 安全](10-security.md)

## 1. 职责与不变量

Gateway 是 Agent 访问数据的**唯一出口**。无状态，可多实例，只连 Agent DB 只读从库。

- **I-GW-1** 没有写接口；没有到管线、源库、控制库写权限的网络路径。
- **I-GW-2** 每个数据请求都绑定一个**最终用户主体**（principal），并以该主体的 `oo_r_<role>` 与 claims 执行。
- **I-GW-3** 应用层检查失败即拒绝（fail closed）；应用层检查被绕过时，数据库权限仍然成立（[05 §4](05-agent-db.md)）。
- **I-GW-4** 每次调用（放行或拒绝）都产生一条审计记录。
- **I-GW-5** 返回给 Agent 的任何内容都不包含凭证、内部主机名、原始 EXPLAIN、或数据库原始错误中可能泄漏隐藏数据的部分。

## 2. 请求处理流水线

```text
HTTP 请求
 │ ① 认证：解析 token → principal{user, role, claims, session_id, sandbox_id, client_kind, limits}
 │ ② 准入：per-token 并发信号量 · per-project 令牌桶 · 全局熔断状态
 │ ③ 解析：pglast（libpg_query，即 PG 自身的语法分析器）→ AST
 │ ④ 静态检查：语句类型 · 可写 CTE · 锁定子句 · 函数白名单 · 系统目录 · 限定名
 │ ⑤ 权限预检：引用的关系与列 ⊆ 该角色可见集合（来自编译后的权限快照）
 │ ⑥ 代价预估：BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY; SET LOCAL ROLE; SET LOCAL oo.claims; EXPLAIN (FORMAT JSON) <sql>
 │ ⑦ 执行：同一事务内 SET LOCAL statement_timeout；预备语句；服务端游标按上限取行
 │ ⑧ 结果：类型化序列化 · 单元格截断 · 行/字节上限 · 截断标记 · 结果哈希
 │ ⑨ 审计：异步写入（失败时本地落盘重试，不丢）
 ▼
响应（含 query_id、freshness、truncated、hint）
```

第 ⑥ 与 ⑦ 在同一个连接、同一个 REPEATABLE READ 只读事务中进行；结果、新鲜度、水位和质量状态共享同一快照，默认 READ COMMITTED 不能保证多条语句使用同一快照。质量元数据由 Gateway 在执行完 Agent SQL 后切换到 `oo_gateway_sys` 读取，再恢复用户角色；该阶段只执行平台固定 SQL。评测模式的 Gateway 实例额外 `SET LOCAL oo.as_of = '<题目的 as_of>'`，使 `oo_dict.calendar.today` 固定在题目的日期（[05 §6.3](05-agent-db.md)）。事务总是以 `ROLLBACK` 结束；连接归还连接池前执行 `RESET ALL`（`SET LOCAL` 已随事务失效，这是额外保险）。

## 3. 身份与 token

### 3.1 两类 token

| token | 持有者 | 签发 | 形态 | 有效期 | 绑定 |
|---|---|---|---|---|---|
| **会话 token** | 出口代理（沙盒内不可见） | Agent 入口在建会话时调用 Gateway 内部签发接口（mTLS） | 不透明随机串 `oost_…`，Gateway 只存其哈希 | 与会话相同，默认空闲 30 分钟、绝对上限 8 小时 | `session_id`、`sandbox_id`、最终用户、角色、claims；**只接受经出口代理（mTLS 客户端证书）发来的请求** |
| **外部客户端 token** | 直接接 MCP/SDK/HTTP 的客户端 | MCP：OAuth 2.1 授权码 + PKCE（Control API 作为授权服务器或对接企业 IdP）；SDK/HTTP：管理员签发的服务账号 token | 不透明串 `ooct_…` | 默认 1 小时（OAuth，可刷新），服务账号最长 90 天 | 固定的用户或服务账号、角色；受众（audience）固定为该 Gateway 的规范 URL |

选择**不透明 token + 服务端查找**而非自包含 JWT：Gateway 是唯一的验证方，不需要离线验证；吊销立即生效（会话结束即失效）；token 泄漏时无法被解码出 claims。查找结果在 Gateway 本地缓存 ≤ 5 秒，吊销时通过 Redis pub/sub 主动失效。

### 3.2 主体解析与 claims

```yaml
# roles.yaml（节选，完整格式见 12 配置参考）
roles:
  dispatcher_eu:
    grants:
      - schema: logistics
        tables: ["*"]
        exclude: ["logistics.receivable*"]
      - table: logistics.customer
        columns: { exclude: [phone, id_card] }
    row_filters:
      logistics.shipment: "region = ANY (_oo.oo_claim_array('region'))"
    limits: { max_rows: 2000, export_rows: 500000, concurrency: 2 }
bindings:
  - match: { idp_group: "eu-dispatch" }
    role: dispatcher_eu
    claims:
      region: "{{ user.attributes.region }}"     # 取自 IdP 声明，签发时代入
```

- 一个主体只有**一个**有效角色（避免多角色合并时的权限推理歧义）。需要更大权限时，定义一个组合角色。
- claims 在签发时代入并冻结在 token 记录中；Agent 无法改变。
- 当 IdP 中的用户属性变化时，新会话生效；存量会话在下一次 token 刷新（外部 token）或会话结束时生效。管理员可强制吊销某用户的所有 token。

### 3.3 与 MCP 授权规范的对齐

- Gateway 的 MCP 端点作为 OAuth 资源服务器，发布 RFC 9728 受保护资源元数据；要求客户端使用 RFC 8707 resource indicator，token 的 audience 必须等于本资源 URL。
- **禁止 token 透传：** MCP Server（[08](08-agent-interface.md)）调用 Gateway HTTP API 时，不转发它收到的用户 token，而是用 RFC 8693 token exchange 换取一个受众为 Gateway 的内部 token。

## 4. SQL 静态检查

### 4.1 解析器选择：pglast 为准，sqlglot 为辅

| 用途 | 工具 | 原因 |
|---|---|---|
| 安全判定（语句类型、引用关系、函数） | **pglast**（libpg_query） | 使用 PostgreSQL 自身的语法分析器，“解析器认为的语句”与“数据库将执行的语句”严格一致，消除解析差异导致的绕过 |
| 友好提示（例如 Agent 写了 MySQL 方言） | sqlglot | 能识别多方言并给出改写建议；**不参与放行判定** |

pglast 解析失败 = 数据库也不会接受 = 直接返回 `SQL_PARSE_ERROR`，并附 sqlglot 给出的方言提示（如“`LIMIT a, b` 是 MySQL 写法，PostgreSQL 请用 `LIMIT b OFFSET a`”）。

### 4.2 规则

| # | 规则 | 说明 |
|---|---|---|
| R1 | 只允许**一条**顶层 `SelectStmt` | `WITH ... SELECT` 在 AST 中也是 `SelectStmt`；拒绝 `EXPLAIN`、`SHOW`、`SET`、`COPY`、`DO`、`CALL`、`PREPARE`、`LISTEN` 等一切其他语句 |
| R2 | 递归检查所有 CTE 与子查询，任何位置出现 `InsertStmt`/`UpdateStmt`/`DeleteStmt`/`MergeStmt` 即拒绝 | 防可写 CTE：`WITH x AS (DELETE ... RETURNING *) SELECT ...` |
| R3 | 拒绝 `FOR UPDATE/SHARE/NO KEY UPDATE/KEY SHARE`、`SELECT INTO` | 锁与建表 |
| R4 | **函数白名单**（而不是黑名单） | 白名单由 `pg_proc` 生成：常用聚合、数学、字符串、日期时间、JSON、窗口函数、`pg_trgm` 相似度函数；显式排除所有 `VOLATILE` 函数（`random()` 等少数除外）、`pg_*` 管理函数、`set_config`、`current_setting`（由 `_oo` 函数间接使用）、`pg_sleep*`、`pg_advisory_*`、`dblink*`、`lo_*`、`pg_read_*`、`pg_ls_*`、`txid_*`/`pg_current_xact_id` 等 |
| R5 | 禁止访问 `pg_catalog`、`information_schema`、`_oo`、`*__stage` | Agent 通过 `list_tables`/`describe_table` 获取元数据；`oo_dict`、`oo_entity` 按普通对象授权，可以访问 |
| R6 | 所有关系必须是 schema 限定名 | 连接的 `search_path = ''`；未限定时返回提示“请使用 `logistics.customer`”，并在唯一匹配时给出建议名 |
| R7 | 引用的关系 ⊆ 角色可见集合；显式列 ⊆ 可见列 | 不可见的表返回与“不存在”相同的错误，避免存在性泄露；不可见的列返回 `COLUMN_NOT_ALLOWED` 并列出可用列（尚未完成分类的新列注明“待分类，暂不可用”） |
| R7a | `*` 与 `t.*` 展开为该角色的可见列 | 由于授权一律是列级的（[05 I-DB-5](05-agent-db.md)），PG 对 `SELECT *` 要求全部列都有权限，任何一张表只要有不可见列就会报错。Gateway 在 AST 上把星号展开为可见列（按表定义的列顺序）后再 deparse 执行，这是唯一的一处 SQL 改写，语义与“只能看到这些列”一致；响应中注明“`*` 已展开为 N 列” |
| R8 | 语句长度 ≤ 64 KB；AST 节点数 ≤ 上限；递归 CTE 允许但受超时约束 | 防解析层 DoS |
| R9 | 拒绝对大表（行数估计 > 阈值）的无过滤 `SELECT *` | 返回提示：加时间条件，或改用 `export` |

星号展开只处理 SELECT 投影，不改写 `count(*)`。必须按作用域解析表别名、CTE、子查询和 JOIN（含 USING 的合并列），保持输出列顺序；无法安全展开或没有可见列时拒绝执行。展开后的 AST 重新执行门禁，R9 根据原始投影判定，不能因展开而绕过大表限制。

### 4.3 数据库层同步加固（`oo apply` 执行）

```sql
REVOKE EXECUTE ON FUNCTION pg_sleep(double precision), pg_sleep_for(interval), pg_sleep_until(timestamptz) FROM PUBLIC;
REVOKE EXECUTE ON FUNCTION set_config(text, text, boolean) FROM PUBLIC;
REVOKE EXECUTE ON FUNCTION pg_advisory_lock(bigint), pg_advisory_lock_shared(bigint) /* …全部 advisory 函数… */ FROM PUBLIC;
-- dblink / postgres_fdw / file_fdw 不安装；lo_* 与 pg_read_* 默认仅超级用户或预定义角色可用，确认 oo_r_* 不是这些角色的成员
ALTER DEFAULT PRIVILEGES REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;   -- 此后新建函数默认不可执行
```

兜底测试（[13](13-evaluation-and-benchmarks.md) 轨道 B）会关闭 ③–⑤ 全部应用层检查，直接以 `oo_r_*` 执行攻击用例集，要求所有写入、改 claims、越权读取、服务端文件访问用例失败。

## 5. 代价预估与读取控制

### 5.1 EXPLAIN 准入

执行前在同一事务中执行 `EXPLAIN (FORMAT JSON)`，读取根节点的 `Total Cost`、`Plan Rows`，并遍历计划节点：

| 条件 | 默认阈值 | 结果 |
|---|---|---|
| `Total Cost` > 硬上限 | 按从库规格校准（原型中以 5,000 万行表标定） | `COST_TOO_HIGH` + 提示 |
| 对 > 1,000 万行的表做 `Seq Scan` 且无过滤 | — | `COST_TOO_HIGH`，提示“加上时间范围条件或使用 {索引列}” |
| `Nested Loop` 的两侧行数估计乘积 > 阈值 | 1e9 | `COST_TOO_HIGH`，提示“检查 JOIN 条件是否遗漏（疑似笛卡尔积）” |
| 估计返回行数 > `max_rows` × 100 且非聚合 | — | 放行，但在响应中提示“结果将被截断，建议聚合或用 export” |

**EXPLAIN 只是软门禁：** 估计值会严重偏离实际（数据倾斜、`LIMIT`），所以真正的硬边界是 `statement_timeout`、`temp_file_limit`、`work_mem` 和并发上限。代价阈值只用于提前拒绝明显的坏查询并给出可操作的提示。原始计划不返回给 Agent（会泄漏被 RLS 隐藏的行数规模），只返回结论。

### 5.2 执行与截断

- `SET LOCAL statement_timeout` 按请求类型：`query` 默认 30s，`export` 默认 120s，`search_values` 3s。
- 用服务端游标取 `max_rows + 1` 行，以判断是否截断，**除 R7a 的星号展开外不改写 Agent 的 SQL**（不外包 `LIMIT`，避免改变语义，例如 `ORDER BY ... LIMIT` 已有时）。
- 截断维度：行数（默认 200，按角色可配到 2,000）、总字节（默认 256 KB）、单元格（文本 > 500 字符截断并标注）。
- 响应中包含：`truncated`、`returned_rows`、`limit`、以及 `estimated_total_rows`（来自 EXPLAIN，明确标为估计值）。

### 5.3 导出模式

| 项 | `query` | `export` |
|---|---|---|
| 返回 | JSON 行（或 Markdown 由客户端渲染） | Arrow IPC 流（HTTP chunked），客户端写成 Parquet |
| 行上限 | 200–2,000 | 默认 50 万，按角色可配 |
| 字节上限 | 256 KB | 默认 1 GB |
| 超时 | 30s | 120s |
| 并发权重 | 1 | 4（占用更多信号量额度） |
| 审计 | SQL + 统计 + 结果哈希 | 同左，另记录导出字节数 |

导出同样经过 ①–⑥ 全部门禁；区别只在上限、权重和传输格式。

### 5.4 质量警告

查询结果本身是对的，不代表数据是完整、一致、可信的。Gateway 在执行后（同一事务、同一从库快照内）读取被引用对象的 `_oo.table_quality` 与 `_oo.source_watermark`（[05 §6.2](05-agent-db.md)），把需要告知的状态作为 `warnings` 随结果返回。场景表与视图通过权限快照中预先计算的血缘（`after_load` SQL 解析得到的上游表）映射到底层源表。

| 警告 | 触发条件 | 给 Agent 的说明（示例） |
|---|---|---|
| `RECONCILE_MISMATCH` | 引用的表最近一次对账有差异、尚未修复 | “erp_mysql.ysk 最近一次对账发现 42 行不一致，修复前结果可能有偏差” |
| `RECONCILE_STALE` | 超过 `reconcile_max_age` 未对账 | “erp_mysql.ysk 已 4 天未对账” |
| `VALUES_REJECTED` | 查询引用的**列**有未解决的转换拒绝 | “fhrq 有 312 行原值非法（零日期），已置为空，未参与按日期的筛选” |
| `UNEXPLAINED_CODES` | 查询引用的码值列存在 `undetermined` / `unexplained` 值 | “zt 有 1,204 行为码 7（含义未确认），按类别筛选时未计入” |
| `PARTIAL_TRANSACTION` | 涉及的表有超大源事务正在分段写入 | “ERP 正在写入一笔大批量变更（月结），相关数据尚未完整” |
| `CROSS_TABLE_SKEW` | 源有暂停表，或批量一致性降级；per_table 模式下跨表 JOIN 无一致性保证（位点差用于说明偏差程度） | “当前源未处于跨表一致切面，JOIN 结果可能不一致” |
| `ENTITY_CHECK_FAILED` | 引用了 `oo_entity` 或其成员关系，且实体解析不变量未通过 | “客户合并关系今晚的校验未通过，按客户汇总的结果可能有偏差” |
| `STALE_DATA` | 新鲜度超过该表配置的期望值 | “erp_mysql.ysk 数据截至 3 小时前” |

- 警告是结构化的（`code`、涉及的表/列、数量、说明文本），同时写入审计；
- 质量状态在本次查询的数据库快照中读取，不以跨查询缓存代替，避免缓存遗漏刚提交的分段事务或转换拒绝；
- `AGENTS.md` 要求 Agent 在回答中说明所有警告（[09 §7](09-sandbox-and-agent-runtime.md)），评测中未说明计为失败（[13 §6.4](13-evaluation-and-benchmarks.md)）。

## 6. 并发与过载保护

```text
请求 ─▶ per-token 信号量（默认 2）─▶ per-project 令牌桶（QPS 与“代价单位”两个桶）
     ─▶ 全局加权公平队列（按 project）─▶ 全局连接池（每实例 N，总量 ≤ 从库 CONNECTION LIMIT）
     ─▶ 熔断器（从库健康）
```

- **排队：** 队列等待超过 5s 返回 `RATE_LIMITED`（HTTP 429，`retry_after`），属可重试错误，CLI/SDK 自动退避重试 ≤ 3 次。
- **熔断：** 每 5s 采样从库：活跃连接数、`pg_stat_activity` 中运行 > 20s 的查询数、复制回放延迟、（可选）主机 CPU。超阈值进入半开状态，只放行 `list_tables` 等元数据操作与低代价查询，返回 `OVERLOADED`（503，可重试）。
- **取消：** 客户端断开或会话被结束时，Gateway 对进行中的查询发送取消（`pg_cancel_backend` 语义，由驱动的 cancel 请求实现），释放从库资源。
- **分布式计数：** 令牌桶与信号量计数在 Redis 兼容存储中（Lua 脚本原子操作，带 TTL 防止泄漏）；Redis 不可用时 Gateway 退化为本地限流（按实例数均分额度），不放开限制。

## 7. 错误契约

所有错误使用统一结构，面向 Agent 可读、可行动：

```json
{
  "error": {
    "code": "COST_TOO_HIGH",
    "message": "预估代价过高：对 erp_mysql.ydh（约 4,800 万行）的全表扫描。",
    "hint": "加上时间范围条件，例如 WHERE fhrq >= date '2026-09-01'；该表在 fhrq 上有 BRIN 索引。",
    "retryable": false,
    "query_id": "q_01J9Z…"
  }
}
```

| code | HTTP | 可重试 | 含义 |
|---|---|---|---|
| `UNAUTHENTICATED` | 401 | 否 | token 无效/过期/已吊销 |
| `SQL_PARSE_ERROR` | 400 | 否 | 语法错误（附方言提示） |
| `STATEMENT_NOT_ALLOWED` | 400 | 否 | 非单条 SELECT、可写 CTE、锁子句等 |
| `FUNCTION_NOT_ALLOWED` | 400 | 否 | 函数不在白名单 |
| `RELATION_NOT_FOUND` | 400 | 否 | 表不存在**或**无权访问（刻意不区分） |
| `COLUMN_NOT_ALLOWED` | 400 | 否 | 列不存在或无权访问，附可用列 |
| `UNQUALIFIED_RELATION` | 400 | 否 | 缺 schema 前缀，附建议名 |
| `COST_TOO_HIGH` | 422 | 否 | 代价准入拒绝，附改写建议 |
| `TIMEOUT` | 408 | 否 | 执行超时，附建议 |
| `TABLE_PAUSED` | 409 | 否 | 表同步已暂停（结构变化），附原因与最后同步时间 |
| `QUERY_TOO_SHORT` | 400 | 否 | 名称解析/值检索的输入短于最短长度，附“请提供更完整的名称” |
| `ENUMERATION_REFUSED` | 400 | 否 | 疑似枚举全部实体或值的请求 |
| `RATE_LIMITED` | 429 | 是 | 排队超时 |
| `OVERLOADED` | 503 | 是 | 熔断 |
| `INTERNAL` | 500 | 是 | 内部错误（详情只进审计与日志） |

数据库原始错误只进审计与日志；返回给 Agent 的消息经过模板化（例如类型不匹配、除零错误保留可操作的部分，去掉值内容）。

## 8. 审计

### 8.1 记录内容

| 字段 | 说明 |
|---|---|
| `query_id` | ULID，返回给调用方，Agent 在回答中引用 |
| `ts_start` / `duration_ms` | — |
| `principal` | 用户 ID、角色、claims 哈希、`client_kind`（sandbox / mcp / sdk / http） |
| `session_id` / `sandbox_id` | 与 Agent 入口的会话记录关联 |
| `tool` | query / export / resolve / search_values / describe_table … |
| `warnings` | 本次返回的质量警告（§5.4） |
| `sql` / `fingerprint` | 原文与规范化指纹（pg_query fingerprint） |
| `relations` | 引用的表（解析得出） |
| `decision` / `error_code` | 放行 / 拒绝原因 |
| `plan_cost` / `plan_rows` | EXPLAIN 摘要（不对 Agent 返回） |
| `rows` / `bytes` / `truncated` | — |
| `result_sha256` | 结果的规范化哈希，可用于事后核验“Agent 引用的数字确实来自这次查询” |
| `knowledge_version` / `gateway_version` / `replica_lag_ms` | — |

**不存储结果数据本身**（只存哈希）；需要复现时，用审计中的 SQL 与数据版本重新执行。

### 8.2 存储与保留

- 写入控制库的按日分区表 `audit.gateway_calls`，Gateway 以批量异步方式写入；写入失败时追加到本地 WAL 文件并重试，保证不丢（审计丢失是安全事件）。
- 默认保留 180 天，可配置；支持经 OTLP / syslog 同步输出到企业 SIEM。
- 可选的防篡改：每日对当天记录做哈希链并把根哈希写入对象存储的 WORM 桶。

## 9. 其他 API

| 端点 | 作用 | 门禁 |
|---|---|---|
| `GET /v1/tables` | 列出可见表（来自权限快照 + Knowledge 摘要） | 认证 |
| `GET /v1/tables/{schema.table}` | 结构 + Knowledge 中的说明（按角色编译版本） | 认证 + 可见性 |
| `POST /v1/entities/{entity}/resolve` | 把用户说的名称解析为 `entity_key`（分层规则见 §9.1） | 全套门禁；在用户角色 + RLS 下执行；独立限频 |
| `POST /v1/values/search` | 在非实体的 `searchable` 列（线路编码、仓库名等）上做模糊检索（`pg_trgm`），在用户角色 + RLS 下执行 `SELECT DISTINCT col ... ORDER BY similarity LIMIT 10` | 全套门禁（SQL 由 Gateway 生成）；同 §9.1 的最短长度与防枚举规则 |
| `GET /v1/freshness` | 各可见表的 `_oo.table_freshness`、所属源的 `_oo.source_watermark`、`_oo.table_quality` 摘要与从库回放延迟 | 认证 |
| `GET /v1/knowledge/{path}` / `POST /v1/knowledge/search` | 读取/检索按角色编译的 Knowledge Pack | 认证 |
| `POST /v1/sql/validate` | 只做 ③–⑥，不执行；返回是否可执行与提示 | 同 query（计入限流） |
| `POST /v1/feedback/gap` | Agent 报告知识缺口，进入 Ontology 审核队列 | 认证；限频；内容作为不可信文本存储 |
| `GET /v1/queries/{query_id}` | 调用方查询自己发起过的查询的元数据（不含结果） | 只能看自己的 |
| `POST /internal/v1/session-tokens` | Agent 入口签发/吊销会话 token | mTLS，仅入口 |

### 9.1 实体解析的分层规则

名称解析的目标是“要么唯一确定、可以算钱；要么给一张短名单让人确认”，而不是返回一个“最像的”结果让模型直接往下算。在 `oo_entity.<entity>_aliases` 与规范实体表上依次尝试，**某一层得到唯一结果就停止**：

| 层 | 规则 | 结果 |
|---|---|---|
| 1 精确 | 别名原文整段相等；别名有效（非 `deprecated`、在有效期内——评测模式下以 `as_of` 为准） | 唯一 → `resolved`，`method: exact` |
| 2 规范化 | 双方按实体的 `normalization` 规则处理后相等（全角转半角、忽略大小写、合并空白、去掉“有限公司”等后缀）；规范化后短于 `min_length` 时跳过本层 | 唯一 → `resolved`，`method: normalized`，并返回命中的别名原文 |
| 3 包含 | 别名包含用户说法，或用户说法包含整条别名；用户说法短于 `min_length` 时不做 | 返回 ≤ 8 个候选，`status: ambiguous`；**不自动选择** |
| 4 相似建议 | 前三层都无结果时，`pg_trgm` 相似度 ≥ 0.4 的前 5 个 | `status: not_found`，附 `suggestions`，只用于向用户确认 |

- 返回字段只有 `entity_key`、规范显示名、命中的别名、命中方式；不返回电话、地址、证件号等属性（候选名单会进入模型上下文，也就可能发往模型服务）；
- `ambiguous` / `not_found` 时，`AGENTS.md` 要求 Agent 向用户确认，或在用户已经说清时用 `entity_key` 重新查询，不得自行挑一个继续计算；
- 防枚举：空串、通配符式输入（`%`、`*`、“全部”“所有客户”）直接拒绝（`QUERY_TOO_SHORT` / `ENUMERATION_REFUSED`）；每会话限频（默认 60 次/分钟）；同一会话的不同查询词数量异常时告警；
- 模糊命中从不写回：用户确认“华北钢铁就是华北钢铁销售有限公司”之后，Agent 可以用 `oo gap` 提出“补充别名”的建议，由管家决定是否加入别名陈述；
- 全部在用户的角色与 RLS 下执行：看不到的客户不会出现在候选里。

## 10. 实现要点

- **技术：** Python 3.12、FastAPI（ASGI，uvicorn）、asyncpg（预备语句、二进制协议、取消支持）、pglast（或 [02 §5](02-technology-stack.md) 中的宽松许可替代绑定，二者都基于 libpg_query）、pyarrow。
- **权限快照：** `oo apply` 后由 Control API 生成“角色 → 可见关系/列 → 行级过滤”的只读快照（JSON，带版本），Gateway 启动与收到变更通知时加载；与数据库中的实际 GRANT 定期比对，不一致告警（防手工改库）。
- **性能目标：** 检查 ③–⑤ p99 < 10ms；Gateway 自身开销（不含 SQL 执行）p99 < 30ms。
- **可观测：** OpenTelemetry trace（`session_id`、`query_id` 作为属性）、Prometheus 指标（按 code 的拒绝率、队列深度、熔断状态、从库延迟）。
