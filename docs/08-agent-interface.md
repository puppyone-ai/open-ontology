# 08 Agent 接口：HTTP API、SDK、`oo` CLI 与 MCP

> Status: Proposed
> Owner: Open Ontology Engineering
> Last reviewed: 2026-10-21
> 依赖：[07 Gateway](07-gateway.md)、[06 Ontology](06-ontology-layer.md)
> 被依赖：[09 沙盒与运行时](09-sandbox-and-agent-runtime.md)

## 1. 职责与原则

Wrapper把Gateway的同一组能力包装成不同接入形式，所有授权和数据检查仍在Gateway，Wrapper不增加业务逻辑。这里是Ontology系统的对外交付接口，使用方可以是企业自己的harness、通用Agent客户端或普通程序，接口不依赖Pi的事件格式或内置会话。

CLI适用于能执行命令的调用方，HTTP/SDK适用于能发请求或使用客户端库的调用方，MCP适用于实现对应协议的客户端。Knowledge文件和接口提供同一发布版本的角色视图。各形态按实现进度交付，某一形态通过不代表其余形态已经可用；外接验收使用非内置Pi的真实调用方，核对Knowledge版本、查询结果、权限拒绝和审计。

| 形式 | 运行位置 | 使用方 |
|---|---|---|
| HTTP API | Gateway 自身的接口（OpenAPI 3.1） | IM 机器人、企业系统、其他语言的客户端 |
| Rust SDK / TS SDK | 调用方进程内的客户端库 | 企业自研 Agent；Rust 客户端程序 |
| `oo` CLI | 调用方机器上的命令，Rust 原生二进制，复用 oo-sdk | 能执行命令的外部harness或程序；也是内置沙盒harness的主要接口 |
| MCP Server | Gateway 旁的无状态服务（远程 Streamable HTTP），或本地 stdio 进程 | Claude Code、Codex、Cursor 等不在我们沙盒里的 Agent |

面向 Agent 的接口设计原则：

1. **工具少而正交：** §2 列出目标操作，实际命令以当前版本为准；Knowledge 不能承诺执行面尚不存在的能力。优先补现有元数据、校验与审核链路，不为每个业务问题增加工具。
2. **输出为模型设计：** 截断、时效、查询编号和错误原因可机器读取；错误有安全下一步。无权限不提供绕过方式，语义缺失不伪装成 Gateway 拒绝。
3. **节省 token：** 默认行数上限小（200）；长文本单元格截断；`describe` 默认返回精简版，`--full` 才返回全部列。
4. **可追溯：** 每个数据结果都带 `query_id`，Agent 在回答中引用。
5. **幂等只读：** 所有操作都是读取（`report_gap` 除外，它写入的是审核队列而非数据）。

## 2. 操作一览

### 2.1 当前实现与能力发现

2026-10-21 本仓库 `crates/oo-cli/src/args.rs` 已注册的命令家族为 query、tables、describe、validate/sql validate、freshness、knowledge、cancel、status、operations；具体参数取当前二进制 `--help`。`status` 等不能按管理端目标示例推断参数含义。resolve、search-values、export、gap 不在这一当前 Agent CLI 清单中。

`describe.column_details` 已补充当前角色的运行类型/type_oid，保留原 `columns` 名称数组，返回范围为 Pack 与角色 catalog 的交集。应使用它确定 DATE/TIMESTAMP 字面量，而不是从源 MySQL datetime 猜执行类型。

`describe.query_capabilities` 已提供版本化的运行时 SQL 函数、运算符和字面量签名；仍需用帮助和 `validate` 核验 AST 形状与预算，不能编造 `oo capabilities` 命令。`status: unavailable` 不等于支持。当前 Knowledge 搜索是有限字面片段命中，不是实体解析，未命中或命中上限不代表无相关数据。

### 2.2 目标操作

以下表格及 §3–§7 中的输出/SDK/MCP 示例是目标契约，非当前命令与字段清单。只有已实现且在目标角色验证的部分才能进入当前 Pack 的使用指引。

| 操作 | HTTP | CLI | MCP 工具 | 说明 |
|---|---|---|---|---|
| 列出表 | `GET /v1/tables` | `oo tables [--domain D] [--search S]` | `list_tables` | 可见表 + 一行说明 + 层级（scene/raw/file）+ 新鲜度状态 |
| 表详情 | `GET /v1/tables/{t}` | `oo describe <t> [--full]` | `describe_table` | 列、类型、说明、码表引用、关系、注意事项（按角色编译的 Knowledge） |
| 实体解析 | `POST /v1/entities/{e}/resolve` | `oo resolve <entity> "<名称>"` | `resolve_entity` | 把“RPD”“华北钢铁”解析为唯一 `entity_key`；不唯一时返回 ≤ 8 个候选，要求向用户确认（[07 §9.1](07-gateway.md)） |
| 值检索 | `POST /v1/values/search` | `oo search-values <t.col> "<text>"` | `search_values` | 非实体列的值查找（线路编码、仓库名等） |
| 查询 | `POST /v1/query` | `oo query "<sql>"` / `-f file.sql` | `query` | 只读 SQL，结果截断 |
| 导出 | `POST /v1/export`（Arrow 流） | `oo export "<sql>" -o <path>` | 仅本地 stdio 模式 | 大结果写成 Parquet，只返回路径、行数、列 |
| 校验 | `POST /v1/sql/validate` | `oo validate "<sql>"` | `validate_sql` | 不执行，返回能否执行与提示 |
| 新鲜度与质量 | `GET /v1/freshness` | `oo freshness [t]` | `get_freshness` | 各表数据截至时间、同步状态、源水位与跨表一致性、质量状态（对账、拒绝值、未确认码值、待分类列）、从库延迟 |
| 读 Knowledge | `GET /v1/knowledge/{path}`、`POST /v1/knowledge/search` | `oo knowledge cat/search`（沙盒内直接读文件即可） | `read_knowledge`、`search_knowledge` + MCP resources | 同一个发布版本、按角色编译 |
| 报告知识缺口 | `POST /v1/feedback/gap` | `oo gap "<描述>" [--table t]` | `report_gap` | 进入 Ontology 审核队列 |

## 3. `oo` CLI 输出约定

沙盒里的 harness 靠读命令输出决定下一步，所以 CLI 输出是一份**契约**，有回归测试。

### 3.1 查询成功

```text
$ oo query "SELECT c.display_name, sum(r.amount) AS receipts FROM logistics.receipt r JOIN oo_entity.customer c USING (entity_key) JOIN oo_dict.e_tms_pg__receipt__status s ON s.code = r.status AND s.class = 'confirmed' CROSS JOIN oo_dict.calendar cal WHERE r.received_at >= cal.prev_month_start AND r.received_at < cal.month_start GROUP BY 1 ORDER BY 2 DESC LIMIT 5"

| display_name     | receipts     |
|------------------|--------------|
| 浣熊市警察局      | 1284500.00   |
| 保护伞公司华东分部 | 973210.50    |
| …                | …            |

-- query_id: q_01JB3K7Q4M | rows: 5 | 312 ms
-- freshness: logistics.receipt 截至 2026-10-01 09:12:03+08:00（同步延迟 41s）; 源 tms_pg 跨表一致
-- warning UNEXPLAINED_CODES: logistics.receipt.status 有 37 行为码 X（含义未确认），按类别筛选时未计入
```

此例只展示“已解释 confirmed 类别的小计”，不是完整回款排名。未知状态可能改变总额及第一名时，不得把该结果作为“全部客户实收”。有警告须说明；影响完整性时还要撤销相应认证，不能靠附注保留完整结论。

### 3.1a 实体解析

```text
$ oo resolve customer "华北钢铁"
resolved  entity_key=uscc:91130000XXXXXXXXXX  华北钢铁销售有限公司
          method=normalized  matched_alias="华北钢铁销售有限公司"

$ oo resolve customer "钢铁"
ambiguous（4 个候选，请向用户确认，不要自行选择）:
  uscc:9113…  华北钢铁销售有限公司   matched="华北钢铁销售有限公司"  method=contains
  uscc:9132…  江南钢铁贸易有限公司   matched="江南钢铁"              method=contains
  …
```

### 3.2 截断

```text
…（200 行）
-- query_id: q_01JB3K9ZC1 | rows: 200 (已截断，上限 200) | 1.2 s
-- hint: 结果过大。请在保持业务口径的前提下聚合；仅当当前部署已验证 export 能力时才提示导出，不用截断分组计算全局排名。
```

### 3.3 被拒绝

```text
$ oo query "SELECT * FROM erp_mysql.ydh"
ERROR COST_TOO_HIGH: 预估代价过高：对 erp_mysql.ydh（约 4,800 万行）的全表扫描。
hint: 加上时间范围条件，例如 WHERE fhrq >= date '2026-09-01'；该表在 fhrq 上有 BRIN 索引。优先考虑场景表 logistics.shipment。
-- query_id: q_01JB3KB2XE
```

目标退出码：`0` 成功；`2` 被 Gateway 拒绝（不可原样自动重试）；`3` 可重试错误且自动重试已用尽；`4` 认证/网络问题；`1` 其他。当前版本以实际契约测试为准；必须同时检查结构化 error。允许最多两次有证据的语义等价修复作为目标 Agent 预算，不能换身份/直连库绕过拒绝。

### 3.4 其他约定

- `--json` 输出机器可读 JSON（同 HTTP 响应），便于 Rust/TS 或其他客户端处理。
- `oo export` 默认写入 `/workspace/results/`，输出文件路径、行数、列名与类型、`query_id`；文件旁写一个同名 `.meta.json`（SQL、query_id、freshness），便于之后引用。
- 所有命令的帮助文本（`oo --help`、`oo query --help`）简短且含示例，因为 harness 会读它。
- 可重试错误（`RATE_LIMITED`、`OVERLOADED`）CLI 自动指数退避重试最多 3 次，并在输出中注明。

## 4. HTTP API（节选）

### 4.1 `POST /v1/query`

请求：

```json
{
  "sql": "SELECT …",
  "max_rows": 200,
  "timeout_ms": 30000,
  "format": "json",
  "client_context": { "purpose": "answer", "turn": 3 }
}
```

响应：

```json
{
  "query_id": "q_01JB3K7Q4M",
  "columns": [{"name": "name", "type": "text"}, {"name": "receipts", "type": "numeric"}],
  "rows": [["浣熊市警察局", "1284500.00"]],
  "returned_rows": 5,
  "truncated": false,
  "estimated_total_rows": null,
  "duration_ms": 312,
  "freshness": [{"table": "logistics.receipt", "as_of": "2026-10-01T01:12:03Z", "lag_ms": 41000, "state": "live"}],
  "sources": [{"source": "tms_pg", "consistency": "transactional", "cross_table_consistent": true, "applied_pos": "…"}],
  "warnings": [{"code": "UNEXPLAINED_CODES", "table": "logistics.receipt", "column": "status", "rows": 37,
                "message": "logistics.receipt.status 有 37 行为码 X（含义未确认），按类别筛选时未计入"}],
  "star_expanded": null,
  "knowledge_version": "v12",
  "hints": []
}
```

- `numeric` 以字符串返回，避免 JSON 浮点精度损失；时间统一 ISO 8601 带时区。
- `client_context` 只进审计，便于分析 Agent 行为。

### 4.2 `POST /v1/export`

响应为 `application/vnd.apache.arrow.stream`，HTTP 头携带 `OO-Query-Id`；freshness 和 warnings 在最终查询元数据中。只发送合法 Arrow IPC 流，不追加不同 schema 的 JSON 批次。SDK 流式写临时 Parquet，收到 IPC 结束标记后，再读取自己有权访问的 `GET /v1/queries/{query_id}` 完成记录（`status`、`rows`、`bytes`、`truncated`、`result_sha256`、`freshness`、`warnings`）。校验行数/规范化结果哈希后原子改名，并写 `.meta.json`；HTTP 中断、缺完成记录或不匹配时删除临时文件并报告失败。HTTP trailer 只能作可选提示，不能依赖代理或客户端一定支持它。

### 4.3 版本与兼容

- 路径版本 `/v1`；只做向后兼容的新增，破坏性变更进入 `/v2` 并保留 `/v1` 至少两个发布周期。
- OpenAPI 3.1 文档由 Rust `utoipa` 显式声明生成并发布；SDK 由 OpenAPI 生成基础层、手写易用层。

## 5. SDK

```rust
// 目标 SDK 调用契约：oo_sdk 第一方 crate 尚待实现；不是现有可运行 API。
use oo_sdk::Client;

async fn example() -> Result<(), oo_sdk::Error> {
    // 沙盒内 OO_GATEWAY_URL 指向出口代理，不读取 OO_TOKEN。
    // 外部客户端另用显式配置并从安全凭证存储取 token，禁止输出到日志。
    let oo = Client::from_sandbox_env()?;
    let _tables = oo.tables(Some("finance")).await?;
    let _info = oo.describe("logistics.receipt").await?;
    let who = oo.resolve("customer", "浣熊市警").await?;
    // 只有 who.status == resolved 才能继续；ambiguous/not_found 需澄清。
    let _ = who;
    let _hits = oo.search_values("logistics.route.code", "HB-0").await?;
    let result = oo.query("SELECT amount FROM logistics.receipt LIMIT 10").await?;
    // result.rows 为带 PG 类型信息的值；numeric 为精确十进制字符串。
    let _ = (result.query_id, result.truncated);
    let _path = oo.export(
        "SELECT amount FROM logistics.receipt",
        "/workspace/results/receipts_2026.parquet",
    ).await?;
    let _fresh = oo.freshness(&["logistics.receipt"]).await?;
    Ok(())
}
```

TS SDK 提供同样的方法（`export` 在浏览器环境不可用）。SDK 不缓存数据结果；认证失败不重试；可重试错误按服务端 `retry_after` 退避。

## 6. MCP Server

### 6.1 形态

- **远程：** 与 Gateway 同区部署的无状态服务，Streamable HTTP，由官方 Rust `rmcp` 实现，目标为 MCP 2026-07-28；实际支持版本由锁定 SDK 和协议契约测试确定，通过初始化协商；2025-11-25 客户端兼容需单独验证。作为 OAuth 资源服务器：RFC 9728 元数据、RFC 8707 audience 绑定；调用 Gateway 时以 RFC 8693 换取内部 token，**不透传**客户端 token。
- **本地 stdio：** `oo mcp --stdio`，运行在用户机器上，使用用户本机配置的外部客户端 token；此模式额外提供 `export`（写到本地工作目录）。

### 6.2 工具定义要点

工具描述是模型读到的“说明书”，按以下方式写（节选）：

```yaml
- name: query
  description: >
    在企业数据的只读副本上执行当前 Gateway 能力版本允许的一条 SELECT。
    不承诺 WITH、窗口或全部 PostgreSQL 函数。先用元数据与 Knowledge 确认类型、表和口径，接口以实际部署为准。
    表名必须带 schema（如 logistics.customer）。
    结果最多返回 200 行；需要更多数据时先聚合。返回中的 query_id 需要在回答里引用。
    被拒绝时，按返回的 hint 修改 SQL，不要尝试绕过。
  inputSchema: { type: object, required: [sql], properties: { sql: { type: string }, max_rows: { type: integer, maximum: 2000 } } }
  outputSchema: { $ref: "#/defs/QueryResult" }     # 结构化输出；同时附 Markdown 文本内容
  annotations: { readOnlyHint: true, openWorldHint: false }
```

- **Resources：** `oo://knowledge/README.md`、`oo://knowledge/tables/{schema}.{table}.md` 等，映射到按角色编译的 Knowledge Pack；列表按 principal、角色、策略版本与 Knowledge 版本隔离缓存；不跨权限复用。
- **Prompts：** 提供 `analyze_data` 提示模板，内容与沙盒中 `AGENTS.md` 的工作说明一致，让沙盒外的 Agent 遵循同样的工作方式。

### 6.3 沙盒外客户端的风险提示

直接接 MCP 的 Agent 不在我们的沙盒里，意味着：token 在客户端手里；该 Agent 可能同时连着其他能对外发请求的工具（“致命三要素”：私有数据 + 不可信内容 + 外传通道）。因此：

- 外部客户端 token 默认只能绑定**更小权限**的角色，有效期短，可随时吊销；
- 管理员在签发时看到明确的风险提示；
- 可以按角色禁用外部客户端访问（`roles.<r>.allow_external_clients: false`）。

## 7. 引用与可追溯约定

Agent 在最终回答中按以下格式引用数据来源（写在 `AGENTS.md` 与 MCP prompt 中）：

```markdown
已解释 confirmed 类别内，9 月华东区该客户的收款小计为 128.45 万元 [q_01JB3K7Q4M]。
由于仍有未确认状态，不能据此确认全部实收总额或回款第一名。

数据来源：logistics.receipt、oo_entity.customer；口径：月收款额（按收款日期，已确认，不含退款）；
时间范围：上个月（锚点 prev_month_start = 2026-09-01 至 month_start = 2026-10-01）；
数据截至 2026-10-01 09:12（北京时间）。
数据说明：有 37 行收款状态码含义未确认，未计入；这可能影响总额与客户排名。
```

Agent 入口在展示时把 `[q_…]` 渲染成可点击的“查看 SQL”链接（需要用户对该会话有访问权），并在会话记录里校验：回答中出现的每个 `query_id` 都存在于该会话的 Gateway 审计中，否则标记为“引用无效”（评测中计为可追溯性失败，见 [13](13-evaluation-and-benchmarks.md)）。

## 8. 单库 Agent 最小阅读与评测路径（开发适配器）

一个逻辑数据库只配置一个 Gateway、一个角色与一个覆盖全库的 Pack；导入分片不是 Agent 的数据源选择入口。最简路径是 **`oo knowledge` → `oo tables` → `oo describe` → `oo query`**：

1. `oo knowledge cat README.md` 读取领域地图与版本；用 `oo knowledge search "业务术语"` 定位 `INDEX.md`、`domains/`、表说明、已批准 `glossary.md`，需要时展开实体、关系、码表、指标与示例。沿用 [06 §7](06-ontology-layer.md#7-编译knowledge-pack) 的渐进披露，不新增术语机制。
2. `oo tables` 核对当前角色可见候选表；不要把猜中的 SQL 表名当作成功检索，也不要认为可见表的全部原始字段都已授权。
3. `oo describe schema.table` 核对粒度、实际类型、列与 JOIN 两端，先看已批准口径和码义。没有指标定义时可明确统计授权原始表的行数/去重数，但不能默认为有效客户或成功实收；付款客户、发货客户和部门主体不可互换。
4. `oo validate "SELECT ..."`、`oo query "SELECT ..."` 实际执行；最终引用本轮 query_id，检查截断及 warning。`oo freshness` 的目标应用时间不是源提交时间，未测量源时间和逐表快照不等于共同源位点。

以上示例省略连接参数。现有技能与认证配置见 [`tools/eval-harness/agent-resources/SKILL.md`](../tools/eval-harness/agent-resources/SKILL.md) 和 [评测台使用说明](../tools/eval-harness/README.md)；其内容需随当前能力验收，不因文件存在即证明所有指引有效。domain/search/full 等目标参数不可凭文档拼接。当前已修复部分聚合/别名排序，不再一概按拒绝处理；必要时只有取得**未截断的完整分组结果**才能本地排序，否则不能声称全局排名。

目标 `.agent`/`AGENTS.md` 同步契约：先读能力/版本与逐指标就绪，按业务时区解析半开期间，优先使用已审核 recipe；validate/query 后区分 error、空集、NULL、截断和 warnings。对不支持 SQL 只做有界等价修复；对业务缺口列出缺少的定义/证据和责任对象，而非声称源库无数据。脚本成功、合法 query_id、source_time 未测量均不证明业务完整。验收见[整改 A19–A22](../../docs/semantic-query-chain-remediation-plan.md)。

### 8.1 600+ 表的最小改进建议（待 CLI/BLD 评审）

- 全量 `tables` 即使提高表命中率，也可能把几百个无关表送进上下文；同时报告检索噪声和主动无关 describe 数，不能单看命中率。建议最小新增有上限、稳定排序的表名/领域过滤与分页摘要，保留总数、截断标记、Pack digest 和下一页入口；不能用静默截断伪装完整列表。
- Knowledge 搜索应返回命中文件路径、短摘要及关联的 schema.table，而非要求 Agent 下载全 Pack；README 保持领域入口，INDEX 保持一行一表。是否缺乏这些能力应由实际 CLI/Pack 轨迹确认，不预设整库已经失败。
- 表描述应链接相邻实体、跨域关系文件及两端键、基数与适用限制；跨域关系必须来自全库 S0–S4 的确定性候选并经过审核，而不是只靠提示让模型猜 JOIN。
- 现有 approved glossary 若为空，追溯 S5 候选 → S9 审核 → 编译裁剪，不另建词汇系统。码表缺少已确认绑定时明确未知，不把相似表名当成映射证据。
- CLI 的拒绝响应也应完整传递真实 query_id；成功统计和拒绝凭据要分开。Demo 若只接受成功查询编号，其 grounding 拦截应独立记为回答失败，不得把 Gateway 正确拒绝记成模型已完成回答。

本次没有修改 CLI 实现或 Pack 编译预算；建议须经产品及 BLD 评审后独立实现和回归。


## 库级控制面整合（INT）

库级项目、全局 facts/分批 S5、逐对象审核、一个角色 Pack、库级原子发布和单 Gateway
接入见 [ADR-030](adr/ADR-030-library-control-worker-and-gateway.md)。库级安装预算不授予
数据权限；Gateway 保留只读登录、当前导入向量/库 head/fence 校验和原 SQL 子集。
健康库每表给出 target_observations，但源时间/共同源位点未测量仍为 unmeasured，
不把 all-in-one 主库称为只读副本。真实库覆盖及评测以独立执行证据为准。
