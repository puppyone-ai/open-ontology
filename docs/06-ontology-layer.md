# 06 Ontology 层：规格、自动构建与发布

> Status: Proposed
> Owner: Open Ontology Engineering
> Last reviewed: 2026-10-21
> 依赖：[03 数据源](03-sources-and-ingestion.md)、[05 Agent DB](05-agent-db.md)
> 被依赖：[07 Gateway](07-gateway.md)、[08 Agent 接口](08-agent-interface.md)、[09 沙盒](09-sandbox-and-agent-runtime.md)、[13 评测](13-evaluation-and-benchmarks.md)

## 1. 目标

本层产出可由不同harness消费的语义资产：标准OOS YAML、人工审核overlay、Git版本，以及按角色编译的Knowledge Pack。文件可按已批准角色取得，公开接口可读取同一发布版本；生成和读取这些内容不要求启动Pi或内置对话。动态数据仍经Gateway查询，取得Pack不等于取得数据库账号或额外数据权限。

Ontology层回答Agent写SQL之前必须知道、但schema里没有写的东西：

| Agent 的问题 | Ontology 中对应的元素 |
|---|---|
| “客户”在哪张表？`kh` 是什么？ | **Entity**（业务实体）→ **Dataset**（表/视图）映射；表与字段的 label/description |
| 运单和客户怎么关联？一对多还是多对多？ | **Relationship**（JOIN 路径、基数、推荐 JOIN 方式） |
| `status = 3` 是什么意思？ | **Enum**（码表/状态码释义） |
| “RPD”是哪个客户？这三种写法是不是同一家？ | **Entity Resolution**（稳定的 `entity_key`、别名陈述、管家决定）+ 分层解析（`resolve`） |
| “上季度”“上个结账期”是哪几天？ | **Calendar**（命名时间锚点） |
| “月收款额”怎么算？ | **Metric**（统计口径：表达式、过滤、时间归属、单位、负责人） |
| 有没有类似问题的标准写法？ | **Verified Query**（已验证示例） |
| 数据新到什么时候？ | **Freshness**（运行时从 `_oo.table_freshness` 读取，不写死在 Ontology 里） |
| 哪张表是“推荐”的，哪张是原始副本？ | Dataset 的 `tier` 与 `recommended` |

设计目标：

1. **结构化为真相源：** 用可机检的 YAML（Ontology Spec，下称 OOS）表达，Markdown 与 JSON 都是编译产物。
2. **自动构建草稿：** 系统产出覆盖率高、带置信度与证据的草稿；人只处理“需要判断”的部分。
3. **人工编辑永不被覆盖：** 自动产物与人工编辑物理分离（overlay 模型）。
4. **按角色编译：** 不同角色看到的 Knowledge 不同，Knowledge 本身不能成为越权通道。
5. **发布有门禁：** 与数据库不一致、口径 SQL 跑不通、评测回退，都阻止发布。
6. **渐进披露：** 编译产物按 Agent 的阅读路径组织，控制每个文件的 token 预算。

## 2. 与业界语义层的对比与取舍

| 方案 | 可借鉴 | 不照搬的原因 |
|---|---|---|
| Palantir Foundry Ontology（Object / Link / Action） | 以“业务实体 + 关系”作为 Agent 的思考单位；权限与本体绑定 | 闭源；依赖其数据集成与 Action 体系；我们不做 Action |
| Snowflake Cortex Analyst 语义模型 / Semantic View | `dimensions / time_dimensions / facts / metrics / relationships / verified_queries / synonyms / sample_values` 的字段划分非常贴近 LLM 需求 | 绑定 Snowflake；需手工编写 |
| Databricks Genie + Metric View | “instructions + trusted assets + 示例 SQL”的组合；基准问题集内建 | 绑定 Unity Catalog |
| dbt Semantic Layer / MetricFlow | entity / dimension / measure / metric 的严格建模；指标编译器 | 不将通用编译器/dbt 项目设为首个纵切前提；先交付可验证 recipe，并保留受控长尾 SQL |
| Cube、LookML、Malloy | 成熟的指标与 JOIN 声明方式 | 需要数据团队长期维护；面向 BI 而非 Agent |
| Open Semantic Interchange（OSI） | 厂商中立的语义交换格式 | 作为**导出目标**，不作为内部格式（规范仍在演进） |
| Wren AI MDL、Vanna 训练数据 | Agent 场景下的“模型 + 关系 + 示例”三件套 | 以检索增强为主，缺少发布门禁与权限裁剪 |

**修订结论：** OOS 继续用“口径说明 + 已审核可执行 SQL/recipe + 测试”表达首批指标，但不把任意拼接 SQL 的正确性留给 Agent。recipe 必须在实际角色和 Gateway 子集下验证；复杂去重可由受控场景资产承担。通用语义查询编译器仍是可选扩展，不是关键业务可回答性的前置大工程，见 [ADR-031](adr/ADR-031-business-answerability-and-executable-pack.md) 与 [14](14-roadmap-and-risks.md)。

## 3. Ontology Spec（OOS）v1

### 3.1 目录结构与 overlay 模型

```text
ontology/                              # 位于配置 Git 仓库中
  ontology.yaml                        # 清单：版本、语言、包含的 schema、编译选项
  guides/                              # 人写的自由文本（业务背景、注意事项）
    overview.md                        # 编译进 /knowledge/README.md 的“这个库是什么”
  curated/                             # 人工编辑层（控制台写这里；Builder 从不写）
    datasets/<schema>/<table>.yaml
    entities/<entity>.yaml
    entities/<entity>.resolution.yaml   # 实体解析规则（§3.10）；管家决定存控制库，不在这里
    calendar.yaml                       # 业务日历与时间锚点（§3.9）
    relationships.yaml
    metrics/<domain>.yaml
    glossary.yaml
    enums/<schema>.<table>.<column>.yaml
    examples/<domain>.yaml
  generated/                           # 自动构建层（Builder 写这里；人从不手改）
    datasets/<schema>/<table>.yaml     # 结构、画像、推断的语义类型、LLM 草稿
    relationships.yaml                 # 声明外键 + 推断关系（带证据）
    enums/...                          # 候选码表
    candidates/                        # 候选术语、候选指标、候选示例（未经审核）
    evidence/                          # 画像原始数据、源视图定义、查询日志挖掘结果
  reviews/
    decisions.yaml                     # 审核决定：接受/拒绝某个自动条目（可追溯）
```

**合并规则（编译时）：** `effective = merge(generated, curated)`，字段级合并，`curated` 优先；列表按 `name` 键合并。

- `curated` 中 `status: rejected` 的条目会从 effective 中移除（例如错误推断的关系）。
- 当 `generated` 中某字段的**上游事实**变化（例如列类型、码表出现新值），而 `curated` 覆盖了该字段时，编译器生成一个 **stale override** 审核项，而不是静默沿用。
- 只有 `review.state = approved` 或 `provenance = declared`（来自源库的外键、注释等确定事实）的条目默认进入发布版本；低置信的 LLM 草稿进入发布需显式配置 `publish.include_unreviewed: [descriptions]`，且在编译产物中标注“未审核”。

### 3.2 `ontology.yaml`

```yaml
spec_version: oos/v1
name: acme-logistics
locale: zh-CN                    # 描述文本的主语言；synonyms 可多语言
include_schemas: [logistics, erp_mysql, tms_pg, files_finance_2026]
default_tier_preference: [scene, raw]   # Agent 优先查场景层
publish:
  include_unreviewed: [column_descriptions]   # 其余类型必须审核
  require_eval: [public-smoke, customer-core] # 发布前必须通过的评测集
classification:
  auto_accept: rule_high_confidence  # 只有规则引擎高置信判定为 pii: none 的列可自动接受；其余需人工确认（§4.10）
lifecycle:
  deprecation_min_releases: 1        # 公开名称至少以 deprecated 状态保留一个发布版本才能删除（§3.11）
compile:
  per_role: true
  budgets: { readme_tokens: 2000, table_file_tokens: 3000 }
  sample_values: { default: masked, max_per_column: 8 }
```

### 3.3 Dataset（表 / 视图）

```yaml
# curated/datasets/erp_mysql/kh.yaml 与 generated/... 合并后的 effective 视图
dataset: erp_mysql.kh
source_name: "ERP.KH"                      # 源库原名（规范化前）
label: 客户主档（ERP 原始表）
description: >
  ERP 系统的客户主档，一行一个客户账号。已停用客户 zt='9' 仍保留。
  分析请优先使用 logistics.customer（已合并别名、过滤测试客户）。
tier: raw                                  # raw | scene | file
recommended: false
superseded_by: logistics.customer
grain: 每个客户账号一行
primary_key: [khid]
entity: customer
row_count_estimate: 184233                 # generated，来自 pg_class.reltuples
time_columns:
  - { name: cjsj, role: created_at }
  - { name: xgsj, role: updated_at }
columns:
  - name: khid
    label: 客户 ID
    data_type: bigint
    semantic_type: identifier
  - name: khmc
    label: 客户名称
    data_type: text
    semantic_type: entity_name
    searchable: true                       # 建 pg_trgm 索引，供 search_values
    pii: none
  - name: lxdh
    label: 联系电话
    semantic_type: phone
    pii: personal                          # 编译时不输出样本值；角色可按列撤权
    classification: { state: confirmed, by: ops.wang, at: 2026-10-11 }   # 未分类的列不授权（§4.10）
  - name: zt
    label: 客户状态
    semantic_type: status_code
    enum: erp_mysql.kh.zt                  # 引用码表
  - name: khlx
    label: 客户类型（旧）
    status: deprecated                     # 公开名称生命周期（§3.11）
    replaced_by: erp_mysql.kh.khfl
    deprecated_at: ontology/v14
  - name: xyed
    label: 信用额度
    semantic_type: money
    unit: CNY
    description: 单位为元，含税。0 表示未授信而非无限额度。
    provenance: human
    review: { state: approved, by: finance.li, at: 2026-10-12 }
caveats:
  - 2019 年前的数据存在重复客户，已在 logistics.customer 中合并。
source_semantics:                          # 可选，生成层；由连接器的 semantic_caveats 与捕获方法填充
  family: mysql
  product: MySQL
  version: "8.0"
  capture: c1_cdc
  grade: T
  caveats: [source_collation_ci, trailing_space_insensitive]
```

`source_semantics` 是 `oos/v1` 的追加式可选字段，记录来源家族、版本、捕获方法、一致性等级与源语义 caveats（定义见 [database-support/02 §6](database-support/02-connector-architecture.md)）；Builder 把它写入描述起草的输入，Knowledge Pack 在表页渲染“来源与语义”，缺省时按未知处理。

**语义类型（`semantic_type`）枚举：** `identifier`、`foreign_key`、`entity_name`、`code`、`status_code`、`category`、`boolean_flag`、`money`、`quantity`、`ratio`、`event_time`、`date`、`audit_time`、`geo`、`address`、`phone`、`email`、`person_name`、`free_text`、`json`、`system`。它们驱动编译（例如 `money` 必须带 `unit`）、画像（例如 `status_code` 必须有码表）和 PII 默认策略。

### 3.4 Entity

```yaml
entity: customer
label: 客户
synonyms: [客户, 货主, 托运人, shipper, consignor]
description: 与我司签约、下单的企业或个人。一个客户可有多个结算账户。
primary_dataset: oo_entity.customer        # 实体解析物化出的规范实体表（§3.10）
key: entity_key
display: display_name
members:                                   # 哪些源表的行属于这个实体
  - { dataset: erp_mysql.kh, key: [khid], name: khmc, strong_key: nsrsbh, phone: lxdh }
  - { dataset: tms_pg.shipper, key: [id], name: name, strong_key: tax_id }
resolution: customer_resolution            # 解析规则，见 §3.10
datasets: [logistics.shipment, logistics.receipt]   # 以 entity_key 关联本实体的其他表
```

### 3.5 Relationship

```yaml
relationships:
  - name: shipment_to_customer
    from: { dataset: logistics.shipment, columns: [customer_id] }
    to:   { dataset: logistics.customer, columns: [customer_id] }
    cardinality: many_to_one               # one_to_one | one_to_many | many_to_one | many_to_many
    join_hint: left                        # 推荐 JOIN 方式
    provenance: inferred                   # declared_fk | inferred | view_definition | query_log | human
    confidence: 0.97
    evidence:
      containment: 0.9993                  # 子表非空值落在父表键中的比例（抽样）
      name_match: exact
      type_compatible: true
      seen_in_query_log: 42
    review: { state: approved, by: ops.wang, at: 2026-10-11 }
```

### 3.6 Enum（码表）

```yaml
enum: erp_mysql.ysk.zt                     # 应收状态
owner: finance                             # 码值含义由 owner 批准
classes:                                   # 该码表的业务类别；指标按类别引用码值，而不是写死码值
  open:   应收仍开放（可能有余额）
  closed: 已结清
  void:   作废，不是应收
values:
  - { code: "1", label: 待收款,  class: open,   status: approved, source: dictionary_table }
  - { code: "2", label: 部分收款, class: open,   status: approved, source: dictionary_table }
  - { code: "3", label: 已收讫,  class: closed, status: approved, source: dictionary_table }
  - code: "4"
    label: 已核销
    class: closed
    status: approved
    source: inferred                       # 由数据特征推断（§4.6），经 owner 批准
    evidence: { signature: "amount>0 AND received_date IS NULL AND received=amount", rows: 1832 }
    valid_from: 2019-01-01                 # 含义生效区间；同一码在不同年代含义不同时写多行
    review: { state: approved, by: finance.li, at: 2026-10-12 }
  - { code: "5", label: 已作废, class: void, status: approved, source: inferred, review: { state: approved, by: finance.li } }
  - { code: "6", status: undetermined }    # Builder 提出过推断但 owner 尚未批准
coverage: 0.97                             # generated：数据中出现的值被“已批准”解释的行占比
unexplained_values: ["7"]                  # generated：数据中出现、码表中完全没有的值
```

**码值状态与安全默认：**

| 状态 | 含义 | `class` | 对指标与 Agent 的影响 |
|---|---|---|---|
| `approved` | owner 已批准 | 码表定义的类别 | 正常使用 |
| `undetermined` | 有推断但未批准 | 固定为 `undetermined` | 不归入已确认类别；必要码义未知时阻止完整指标认证，可另报明确限定小计 |
| `unexplained` | 数据中出现但码表中没有 | 固定为 `unexplained` | 同上；码值巡检每小时发现并写入 `oo_dict` 与 `_oo.table_quality`，同时生成审核项 |
| `superseded` / `deprecated` | 历史含义被纠正 / 已弃用 | 保留经批准的历史类别与有效期 | 不删除历史；替代定义不得与旧定义有效期重叠 |

这条规则只保证“不把未知码猜成已确认类别”，不保证指标完整。若查询范围内的未知码可能影响“全部实收”等指标，必须将该指标标为未就绪，不能少算后仍保留完整认证。允许返回明确标为“已解释类别的小计”的局部结果，并报告排除范围；必要时金额影响本身也只能记未知。与该码无关的授权原始统计仍可执行。

`oo_dict` JOIN 是需安装、授权和验证的目标实现，不是 enum 文件天然赋予的能力；具体 recipe 必须通过 §7 的当前角色执行闭包。状态巡检也须与所声明的范围和有效期绑定，不能把本节目标机制当成当前 Library 已实现行为。

**含义随时间变化：** 同一个码在不同年代含义不同时，写多条带 `valid_from` / `valid_to` 的值。按时点查询（例如“上季度末的开放余额”）使用在该时点有效的那条；今天批准的新含义不会改写历史期间的结果，除非它的 `valid_from` 早于该时点。被纠正的错误含义不删除，而是写上 `valid_to` 并标注 `superseded`，防止下次构建把同样的错误推断重新加回来。

### 3.7 Metric（统计口径）

以下是目标 OOS 示意，含 date_trunc/COALESCE 等并非当前 Gateway 全部支持的表达式，不可直接作为当前角色的 verified SQL。2026-10-22 已接通 Library S5 指标候选、effective/YAML 保留及摘要绑定的操作者审核输入；这不等于在线 owner 认证、运行能力验证或独立对账已完成。未审核候选仅在显式启用时以 NOT CERTIFIED 展示；`metrics/availability.json` 给出角色实际可见的指标清单，`required_metrics` 阻止缺少必需指标的构建通过。examples 路径不据此升级。详情见[Metrics 链路修复](../../docs/metrics-pipeline-recovery-20261022.md)。优先复用已支持 SQL 或受控场景资产，不以实现通用指标编译器为首个业务闭环的前提。

```yaml
metrics:
  - metric: monthly_receipts
    label: 月收款额
    synonyms: [回款, 月回款, 收款金额]
    description: 按**收款日期**归属月份的已确认收款金额，不含退款。
    owner: finance
    certified: true                        # 只有 owner 审核后才能为 true
    unit: CNY
    status: active                         # 生命周期（§3.11）
    grain: [month]                         # 最细粒度；分组不能比它更细
    time_dimension: logistics.receipt.received_at
    default_anchor: prev_month_start       # 默认时间锚点（§3.9）
    enum_filters:                          # 引用码值一律通过类别，不写死码值（§3.6）
      - { column: logistics.receipt.status, enum: tms_pg.receipt.status, classes: [confirmed] }
    sql: |
      SELECT date_trunc('month', r.received_at) AS month,
             sum(r.amount)                     AS receipts
      FROM logistics.receipt r
      JOIN oo_dict.enum_values ev
        ON ev.enum_name = 'tms_pg.receipt.status' AND ev.code = r.status
       AND ev.class = 'confirmed'
       AND r.received_at::date >= coalesce(ev.valid_from, '-infinity')
       AND r.received_at::date <  coalesce(ev.valid_to, 'infinity')
      WHERE r.received_at >= :start AND r.received_at < :end
      GROUP BY 1
    dimensions: [customer, region, business_line]
    caveats:
      - 按开票日期统计的口径是“月开票额”（metric: monthly_invoiced），不要混用。
    tests:
      - { params: { start: 2026-01-01, end: 2026-02-01 }, expect: { rows: 1 } }
    control_total:                         # 口径对账（§6 G5b）：用独立路径在源库算同一个数
      as_of: last_closed_period_end        # 固定在已结账期间，避免与在途变更互相干扰
      source: tms_pg                       # 在源库只读从库上执行（经连接器，限速）
      sql: |
        SELECT sum(amount) FROM public.receipt
        WHERE status IN ('C')              -- owner 维护的独立写法，不复用上面的 SQL
          AND received_at >= :period_start AND received_at < :period_end
      tolerance: { abs: 0.01 }
```

**口径的两条硬规则：**

1. **码值按类别引用。** 认证指标中凡是过滤状态码、类型码的地方，必须通过 `enum_filters` 声明类别，并在 SQL 中 JOIN `oo_dict.enum_values`；`oo ontology lint` 拒绝在认证指标里写死码值的否定条件（`status <> '9'`、`status NOT IN (...)`）——否定条件会把未释义的新码算进去，正是要避免的情况。
2. **口径要能对上独立计算的控制总数。** SQL 能执行不代表口径没变。每个认证指标带一条 `control_total`：由 owner 用独立写法在源库（或另一条独立路径）上计算同一个数，在固定的已结账锚点上比对。契约变更时、每次发布前、以及生产上每晚各跑一次；不一致时指标降级为“未通过对账”，Agent 使用时必须说明（§6 G5b）。

### 3.8 Glossary 与 Verified Query

```yaml
# glossary.yaml
terms:
  - term: 在途
    definition: 已揽收、未签收的运单。
    maps_to: { dataset: logistics.shipment, filter: "status IN ('picked','in_transit')" }
  - term: 逾期
    definition: 到期日早于锚点日、且在锚点日仍有余额的开放应收；当天到期不算。
    maps_to: { metric: overdue_remaining }
    owner: finance
    status: approved                       # draft | approved | deprecated；只有 approved 的术语编译进 Knowledge
# 客户别名（如“RPD”）不写在术语表里，而是作为实体解析的别名陈述（§3.10），物化到 oo_entity.customer_aliases

# examples/finance.yaml
examples:
  - id: ex-fin-001
    question: 上个月华东区回款最多的 10 个客户
    sql: |
      SELECT c.name, sum(r.amount) AS receipts ...
    uses: [metric:monthly_receipts, rel:receipt_to_customer]
    verified_by: finance.li
    verified_at: 2026-10-12
    last_checked: { at: 2026-10-20, status: ok, result_hash: "sha256:..." }
```

### 3.9 Calendar（业务日历与时间锚点）

“上季度”“近 30 天”“上个结账期”这类说法，交给模型自己推算日期会出错（时区、财年、结账期）。Ontology 定义命名锚点，物化为 `oo_dict.calendar`（[05 §6.3](05-agent-db.md)），Agent 与指标只引用锚点名：

```yaml
# curated/calendar.yaml
business_tz: Asia/Shanghai
fiscal_year_start_month: 4
week_start: monday
anchors:                                   # oo_dict.calendar 的列
  - today                                  # 按业务时区滚动
  - month_start
  - prev_month_start
  - prev_month_end
  - quarter_start
  - prev_quarter_start
  - prev_quarter_end
  - fiscal_year_start
  - prev_fiscal_year_end
  - last_closed_period_end                 # 由财务在控制台结账时更新，不随日期自动滚动
phrases:                                   # 常见说法 → 锚点区间，编译进 glossary.md
  上个月: { start: prev_month_start, end: month_start, bounds: "[)" }
  上季度: { start: prev_quarter_start, end: quarter_start, bounds: "[)" }
  近30天: { start: "today - 29", end: "today + 1", bounds: "[)" } # 含今天的 30 个业务日历日期
  本月至今对比上月同期:
    current: { start: month_start, end: "today + 1", bounds: "[)" }
    previous: { start: prev_month_start, end: "min(month_start, prev_month_start + (today - month_start) + 1)", bounds: "[)" }
```

规则：统计已结账期间的数（逾期、收款、核销）默认用 `last_closed_period_end`，滚动窗口用 `today`；回答中写明用了哪个锚点及其日期（[09 §7](09-sandbox-and-agent-runtime.md)）。

### 3.10 实体解析（Entity Resolution）

陈旧库里同一个客户有多种写法、多个账号、跨多个系统。“按客户汇总”如果把三种写法当成三个客户，金额会少掉另外两套单据；如果把两家不同的公司合并了，又会多算。实体解析把源行映射到稳定的 `entity_key`，是跨表、跨库统计正确的前提。

```yaml
# curated/entities/customer.resolution.yaml
resolution: customer_resolution
entity: customer
key_scheme:                                # entity_key 生成规则；一经发出不回收、不复用
  strong: "uscc:{strong_key}"
  singleton: "src:{source}:{pk}"
strong_key:
  validator: cn_uscc                       # 内置校验器：统一社会信用代码长度与校验位；可插拔
  placeholders: ["", "无", "-", "000000000000000000", "N/A"]
auto_merge: [same_valid_strong_key]        # 唯一的自动合并规则：强键相同、合法、非占位，且没有“不是同一实体”的否定决定
candidates:                                # 只进入管家队列，从不自动合并
  - same_phone
  - normalized_name_equal
  - name_containment
normalization:                             # 同时用于候选生成与别名解析（07 §9.1）
  fullwidth_to_halfwidth: true
  casefold: true
  collapse_whitespace: true
  strip_suffixes: [股份有限公司, 有限责任公司, 有限公司, 公司, "Co., Ltd.", Ltd., Inc.]
  min_length: { han: 2, latin: 3 }
survivorship:                              # 规范实体表的属性取值规则
  display_name: [steward_canonical, longest_member_name]
  phone: { order: [erp_mysql, tms_pg], on_conflict: list_all }   # 不一致时全部列出，不悄悄挑一个
```

**规则与决定分开存放：**

- 规则（上面的 YAML）在 Git 中，走审核与发布；
- 管家的决定（合并、拆开、“不是同一实体”、规范名、别名的等级与有效期）是高频的运营数据，存在控制库 `entity.decisions`，带操作人和时间，写入即审计；实体解析任务在几分钟内把它们物化到 `oo_entity`（[05 §6.4](05-agent-db.md)），不需要发布 Ontology。决定每天导出一份快照到 Git，便于差异审阅。

**管家队列：** 每条候选并排显示两边的名称、强键、电话、来源与命中规则；动作只有两个——“合并”或“不是同一实体”。否定决定永久生效：之后的匹配不会再把这一对送回队列，改一个电话号码也不会重新打开。

**拆开错误合并：** 被拆出的源行获得新键（有合法强键且无否定决定时用强键，否则用单例键）；自动写入一条否定决定防止下次合并回去；成员关系的旧记录写上 `valid_to` 而不是删除；订单、应收等仍挂在各自的源单上，只是通过成员表改指到新键，汇总自动按新键重算。

**别名作为陈述：** 每条别名带原文、规范化形式、等级（`preferred` / `normal` / `deprecated`）、有效期与出处（源系统、单据号或管家）。解析时只用有效期内的非 `deprecated` 别名；`deprecated` 保留，防止同一个错误别名被自动加回来。

**不变量（每晚校验，失败则相关表带质量警告）：** 每个源行恰好属于一个 `entity_key`；按 `entity_key` 汇总的金额类度量等于对应源行之和。

**Builder 的角色：** 从画像中识别强键候选列（格式与校验位命中率）、提出规范化规则中的后缀表、预先计算候选对的规模；是否启用自动合并、用哪一列作强键由人决定。

### 3.11 公开名称的生命周期

Agent 看到的名称（Dataset、列、指标、码表类别、实体、术语）一旦发布，就可能被已验证示例、评测题、企业自研 Agent 的保存查询引用。因此：

- 名称只追加：改名 = 新增新名称 + 旧名称 `status: deprecated`、`replaced_by` 指向新名称；
- `deprecated` 的名称在 Knowledge 中保留，并注明“已弃用，请改用 X”，至少保留 `lifecycle.deprecation_min_releases` 个发布版本；
- 删除需同时满足：已弃用足够版本、没有已验证示例 / 认证指标 / 评测题仍引用它（G12）；
- **指标的语义变更是一次新版本，而不是原地修改：** 聚合方式、过滤条件（含 `enum_filters` 的类别）、粒度、时间维度、默认锚点任一变化，发布校验都会基于保留字面量的规范化 SQL AST 与结构化字段计算语义差异；pg_query fingerprint 只辅助识别结构，不能单独判断含码值、日期、阈值变化的 SQL 是否等价。变更标为“必须由 owner 审核”（G11），并列出受影响的示例与评测题。

### 3.12 JSON Schema 与校验

`crates/oo-core` 以 Rust Serde 类型与 Schemars 定义 OOS，并导出 JSON Schema（`schemas/oos-v1.json`），供控制台表单、编辑器补全与 CI 校验使用。用 `serde-saphyr` 解析 YAML、`jsonschema` 校验输入，再做跨对象引用和业务规则检查；schema 与序列化设置（枚举、默认值、未知字段）必须一致。`oo ontology lint` 执行 §6 中与数据库无关的静态检查。

## 4. 自动构建（Self-build）流水线

### 4.1 总览

```text
触发：table_ready · schema_changed · 定时（每周）· 手动 · 会话反馈累计
  │
  S0 采集元数据 ── Agent DB catalog + 源库原生元数据（注释、外键、索引、视图/存储过程定义）
  S1 数据画像   ── 选中表/列完整聚合观测 + 绑定/预算门禁；pg_stats 仅辅助
  S2 语义类型   ── 规则候选，敏感信号优先；不以名称自动批准
  S3 关系推断   ── 结构/包含度候选 → 范围/基数/角色证据 → 歧义审核
  S4 码表识别   ── 字典/注释/分布证据 → 范围化绑定候选；不按重合率批准
  S5 LLM 起草   ── 表/字段描述、粒度、注意事项、待问专家的问题
  S6 实体聚类   ── 外键图社区发现 + 命名 → 实体与领域划分、推荐表
  S7 候选指标   ── 查询日志、源视图、金额类字段
  S8 候选示例   ── 问题+SQL → 当前目标角色/能力下验证；保留失败及正确空结果
  S9 审核队列   ── 按影响排序的证据卡 → 决定绑定 → effective 投影与覆盖账本
  S10 列分类    ── 新表/新列的 pii 与 semantic_type 分类；未完成分类的列不授权（§4.10）
  │
  分层源资产与审核 → 角色可执行闭包 → 业务对账/问答 → 分层发布门禁（§6）
```

这是修订目标流，不是 S6–S9 当前均已闭环的声明。候选有来源、启发式分数及证据；确定性事实不需要伪造模型置信度。增量复用必须核验完整观测、事实摘要、当前代际/映射以及规则/提示词/模型版本；旧不完整 facts 不得因哈希匹配继续构建。变化只触发受影响对象复核，但不能单凭表名未变沿用审批。

### 4.2 S0 元数据采集

| 来源 | 内容 | 说明 |
|---|---|---|
| Agent DB `pg_catalog` / `information_schema` | 列、类型（映射后）、主键、索引、`reltuples` | 以实际可查询的形态为准 |
| 源库原生元数据（连接器 `read_schema` / `read_logic`） | MySQL `COLUMN_COMMENT`/`TABLE_COMMENT`、PG `COMMENT`、外键约束、源视图定义、存储过程/函数正文 | **源视图与存储过程常常编码了业务 JOIN 与口径**，是高价值证据 |
| 可选：查询日志 | 客户提供的 BI SQL、源库慢日志、`pg_stat_statements` 导出 | 用 Rust `sqlparser` 按源方言提取 JOIN 对、过滤条件、聚合表达式；不支持的语法记录为待审核原文，不得据此执行源存储过程 |
| 可选：已有数据目录 | dbt `manifest.json`、DataHub / OpenMetadata 导出、数据字典 Excel | 作为 `provenance: imported` 的证据导入 |

### 4.3 S1 数据画像

完整性事实与优化统计必须分开。当前 PostgreSQL Library 路径要求对明确选中的每张表、每个字段取得受预算约束的真实聚合观测，保留空表、全 NULL、失败、未观测及绑定代际，缺项阻止依赖该事实的新构建。已实施范围及测试见[观测 P0](../../docs/self-build-observation-plan.md)；其他连接器不能据此自动获准。

PG 的 `ANALYZE` 统计只用于规划和辅助证据，不能冒充精确覆盖率或业务语义：

| 指标 | 来源 |
|---|---|
| 行数估计 | `pg_class.reltuples` |
| 空值率 | `pg_stats.null_frac` |
| 基数 | `pg_stats.n_distinct`（负数表示比例） |
| 高频值 | `pg_stats.most_common_vals` / `most_common_freqs` |
| 分布 | `pg_stats.histogram_bounds` |
| 物理相关度 | `pg_stats.correlation`（决定 BRIN 是否有效） |

以下代表值/格式检查是后续受控扩展：若使用抽样，必须标注方法、范围、预算与隐私授权，不能在全扫描超时后自动降级并仍称完整。当前观测 P0 只新增形态统计，不新增代表原值导出或模型阶段：

- 取值格式识别：手机号、邮箱、证件号、车牌、运单号、金额格式、日期字符串（文本列里存日期是陈旧库的常态）；
- 文本长度分布、是否含 JSON；
- 数值列是否全为整数、是否疑似“分”为单位的金额。

完整观测、抽样和统计估计分别记录；单表一致快照不代表源/目标对账或全库同一切面。证据存受控位置并记录摘要，敏感原值不得随源码入 Git；只有获准的投影按 §7 编译进 Knowledge。

### 4.4 S2 语义类型

规则引擎先行，规则是可解释的候选生成方式，不是业务真值或经过校准的正确率；扫描亦有资源成本。下面是目标候选信号，当前 Library S5 不允许模型更改类型、分类、关系或码义。扩展模型职责须另行实现和验证：

1. 类型规则：`timestamp` → `*_time`；`boolean`/`tinyint(1)` → `boolean_flag`；
2. 命名规则：`*_id`、`*id`、`*_no`、`created_at`/`cjsj`、`updated_at`/`xgsj`；拼音缩写词典（`je` 金额、`sl` 数量、`rq` 日期、`zt` 状态、`dh` 单号/电话…），词典可由社区扩充；
3. 画像规则：低基数 + 短字符串 → `code`/`status_code`/`category`；格式命中 → `phone`/`email`；
4. LLM：输入列名、类型、注释、画像摘要、同表其他列、脱敏样本，输出语义类型与置信度（结构化输出，JSON Schema 约束）。

命名必须按词边界识别，`account` 不等于 `count`；账号等敏感信号优先，数值形态不代表可求和金额。源类型与执行类型分别保留，金额单位、方向和业务时间角色要有独立证据。候选、分类确认与角色授权不能共用一个 auto_accept 决定。

### 4.5 S3 关系推断

候选生成 → 验证 → 打分 → 裁决：

1. **声明外键**（源库约束）：记录 `provenance: declared_fk` 和完整键列。这是结构证据，不给业务含义“置信度 1.0”；仍需核对租户/公司、角色用途和当前有效数据。陈旧库往往没有外键，后面几步补充候选。
2. **候选生成：** 命名匹配（`ydh.khid ↔ kh.khid`、`customer_id ↔ customer.id`）；类型兼容；只以主键/唯一键或高唯一性列为“被引用端”。
3. **包含依赖检验：** 抽样子表的非空取值（例如 1 万个），用索引在父表中查存在比例 `containment`；≥ 0.98 为强候选。检验在 Agent DB 上执行，不触及源库。
4. **证据叠加：** 源视图定义与查询日志中出现的 JOIN 对，直接提高置信度。
5. **基数：** 由两端唯一性得出；多对多通过“桥表”模式识别（两列都是外键、组合唯一）。
6. **歧义审核：** 模型可以建议区分角色，不作最终业务裁决。例如 `create_user_id` 与 `update_user_id` 都指向 `user` 仍是两个不同角色；由范围化证据和有权 owner 确认。

输出的是候选 JOIN 路径，最短路径不等于业务正确路径。认证指标依赖的路径须验证复合键、租户/公司、有效期、父键唯一性、fanout、未匹配/空键和金额守恒；高包含度不能把发货客户升级成付款客户。当前单列推断无法表达的条件应明确阻塞，或转为经审核的场景资产。

### 4.6 S4 码表识别

- 找字典表：形如 `(类型, 代码, 名称)` 的表（`sys_dict`、`t_code`、`*_dict`），画像特征为低行数、高读取引用；
- 把低基数列的取值集合与字典表的代码集合做匹配，Jaccard 高者为候选；
- 解析注释中的枚举描述（“0-待审核 1-已审核 2-驳回”这种写法很常见）；
- **数据特征推断**（字典缺档时的主要手段）：对每个码值统计同行其他列的特征——金额是否为 0、收款日期是否为空、已收是否等于应收、到期日落在锚点之前/之后的比例——得到每个码的“特征签名”，与已解释的码对比，提出“码 4 的签名是‘金额 > 0、无收款日、已收 = 应收’，疑似已核销”这样的候选。候选一律是 `status: undetermined`，附签名与行数作为证据，只有码表 owner 批准后才变成 `approved`；
- **防止取样偏差：** 签名基于全表统计（在从库上按码值分组聚合，限速），不基于抽样；若某个码的行全部落在一个窄时间窗口内（例如到期日全部在过去），提示“样本时间窗口有偏，不足以判断含义”，而不是据此命名（例如不能因为到期日都已过就把它叫作“逾期”）；
- 都失败时，**不让 LLM 猜状态码含义**，而是生成专家问答卡片：“`ydh.zt` 出现 1/2/3/5/9，分别是什么？”附上每个码的签名，帮助专家回忆。

以上签名/多来源核对是目标设计，不是当前 Library S4 已全实现的声明。实际代码集合重合只能产生候选；必须核验字典类型、租户/公司、code/label、启用范围、有效期和一对多冲突。`sort_code` 重合尤其不能作为绑定。审核后需真实投影 enum classes/values 到 effective OOS；只把候选写进队列不算闭环。

### 4.7 S5 LLM 起草

按表调用（大表按列分批），输入为一个结构化“表卡片”：

```json
{
  "dataset": "erp_mysql.ydh", "source_comment": "运单表",
  "columns": [{"name": "fhrq", "type": "date", "comment": null,
               "semantic_type": "date", "null_frac": 0.01,
               "samples_masked": ["2026-03-01", "..."]}],
  "neighbors": ["erp_mysql.kh (via khid)", "..."],
  "view_snippets": ["CREATE VIEW v_ys AS SELECT ... fhrq ..."]
}
```

输出（JSON Schema 约束）：`label`、`description`、`grain`、每列 `label`/`description`/`confidence`、`caveats`、`open_questions[]`。

工程约束：

- **温度 0 + 输入哈希缓存**：缓存命中时复用同一草稿；温度 0 不保证模型跨请求、跨版本逐字节确定，便于 diff 审核，控制成本。
- **模型可配置且可私有化**：Builder 的模型端点与 Agent 使用的模型端点分开配置；企业可指定内网模型。
- **样本值策略**（按数据源配置）：`none`（只发结构与统计）、`masked`（默认：PII 检测后掩码，例如 `138****1234`）、`raw`。PII 检测用规则 + 可插拔检测器（独立 HTTP 检测服务；默认内置 Rust 规则，不依赖特定语言运行时）。
- **提示注入防护：** 数据库注释与样本值是不可信输入。它们被放进明确的数据字段，输出受 JSON Schema 约束，且 LLM 草稿只能写入 `generated/`，不能直接发布。

### 4.8 S6–S8 实体、候选指标与候选示例

- **实体聚类：** 在关系图上做社区发现，结合表名前缀、同义词，提出实体与领域（财务、运输、仓储）划分；多个表描述同一实体时（例如 ERP 客户表与 TMS 托运人表），生成实体解析配置草稿（§3.10）：成员表、强键候选列（按格式与校验位命中率排序）、规范化后缀表，以及“按草稿规则会自动合并多少行、产生多少候选对”的预估，供人决定是否启用。
- **候选指标：** 来源优先级：查询日志中的聚合表达式 > 源视图/存储过程 > `money`/`quantity` 列的朴素聚合。候选指标永远是 `certified: false`，必须由 owner 认证。
- **候选示例：** 按已审核语义生成“问题 + SQL”，经 Gateway 在目标角色及能力版本下验证。报错/超时保留失败证据；空结果可能是正确反例，必须与独立预期比较，不能因为空就丢弃。执行成功仍是 unverified，语义、数值和权限验收通过才进入 verified。未验证示例不编译为可执行指引。

实体/指标/示例需要候选、审核、有效资产、角色投影的完整路径。当前 Library 入口尚未完成此闭环，不能将正式 OOS 示例误当已生成资产。每次转换须提供覆盖账本，输入对象全部进入输出、待审、拒绝或带原因的裁剪之一；grain 等已起草信息不允许静默丢失。

### 4.9 S9 审核队列

审核项按“影响 × 不确定性”排序：

- 影响：表在会话中被查询的频率（来自 Gateway 审计）、是否被指标引用、是否 `recommended`；
- 不确定性：`1 - confidence`、stale override、未解释的码值、多条 JOIN 路径。

控制台提供三种审核视图：逐表审核、专家问答卡片（面向不写 SQL 的业务人员，一次一个问题）、差异审核（与上个发布版本相比变化了什么）。每个决定写入 `reviews/decisions.yaml`，带审核人与时间。实体解析的管家队列是独立视图（§3.10），其决定存控制库。

### 4.10 S10 列分类（授权的前提）

**只有完成分类的列才会被授权给任何角色**（[05 I-DB-5](05-agent-db.md)、[04 I-PL-8](04-pipeline.md)）。分类的最小内容是 `pii`（`none` / `personal` / `sensitive`）与 `semantic_type`。

| 分类来源 | 条件 | 状态 |
|---|---|---|
| 经验证的确定性分类规则 | 证据充分且无敏感/冲突信号，策略明确允许该规则；名称和数值类型本身不够 | 可完成非敏感分类，但仍须独立角色策略授权；不批准业务含义 |
| LLM 判定或规则低置信 | — | `pending`，必须人工确认 |
| 任何被判定为 `personal` / `sensitive` 的列 | — | 人工确认，并由管理员决定哪些角色可见（默认都不可见） |

- 控制台提供“批量确认”视图，但按证据与风险分组确认，不能为赶上线一次批准所有未知列；优先完成首个业务切片必需列，其他列保留待审；
- 发布时，已确认分类的列才进入 `roles.yaml` 中 `tables: ["*"]` 的展开结果；
- 源库后来新增的列（[04 §5.5](04-pipeline.md)）走同一流程，在分类完成、下一次发布之前对 Agent 不可见，Gateway 对它返回 `COLUMN_NOT_ALLOWED`，`_oo.table_quality.hidden_columns` 记录数量。

## 5. 持续演进：从会话中学习

Ontology 的质量主要在上线后提升。数据来源：

| 信号 | 采集方式 | 产生的审核项 |
|---|---|---|
| Agent 报告的知识缺口 | Agent 调用 `report_gap`（见 [08](08-agent-interface.md)），例如“找不到 zt=5 的含义” | 码表/描述补充 |
| Gateway 反复拒绝同类 SQL | 审计聚合并隔离复现 | 区分能力缺失、类型元数据丢失、真实越权和成本问题；修对应组件，不一律补提示词 |
| 用户追问“口径不对” | 入口的点赞/点踩与文字反馈 | 指标审核 |
| 评测失败题目 | [13 评测](13-evaluation-and-benchmarks.md) 的错误归因 | 按归因类别（选错表、JOIN 错、口径错、码值错）生成 |
| 高频查询模式 | Gateway 审计中重复出现的 SQL 结构 | 候选示例、候选场景表、索引建议 |

**Agent 不能直接修改 Ontology。** 所有信号只进入审核队列，由人决定是否采纳——防止提示注入通过“反馈”污染语义层。

## 6. 发布门禁

发布保存 Git tag（`ontology/v<N>`）及不可变内容寻址产物。技术发布和逐指标业务认证分别记状态；目标门禁如下，现有实现未覆盖项不能默认为通过。逐项执行方法见[整改验收矩阵](../../docs/semantic-query-chain-remediation-plan.md)：

| # | 检查 | 失败处理 |
|---|---|---|
| G1 | OOS 静态校验（JSON Schema、引用完整、命名唯一、`money` 必带 `unit` 等） | 阻止 |
| G2 | 与 catalog 一致：引用的表/列存在，类型与 `data_type` 一致 | 阻止 |
| G3 | 关系范围、键、基数、有效期、未匹配/fanout 和度量守恒成立；包含度只作证据 | 失败阻止依赖该关系的指标认证，保留其他可用范围 |
| G4 | 指标所需码义在声明范围内完整且已批准；新码、冲突与金额影响有证据 | 必要未知码阻止完整指标认证；只能明确发布局部小计，不能用 95% 覆盖代替完整 |
| G5a | 指标定义已审；在实际目标角色、Pack、策略和能力版本下 validate/执行成功，tests 通过；enum_filters 有真实可执行映射 | 阻止该角色的 certified 指标 |
| G5b | 指标口径对账：`control_total` 在其锚点上与指标结果一致（容差内） | 阻止发布；生产中每晚失败则该指标降级为“未通过对账”并告警 |
| G6 | 全部 verified 示例及操作指引依赖可见且可执行；固定夹具核数值，动态数据按独立约定核对而非任意更新哈希 | 执行/引用闭包失败阻止；正确空结果也可通过 |
| G7 | 权限引用：`roles.yaml` 中的表、列、过滤列都存在；场景表的行级过滤已声明 | 阻止 |
| G8 | PII：`pii != none` 的列在任何角色的编译产物中都没有样本值 | 阻止 |
| G9 | 预算：每个编译文件不超过 token 预算 | 阻止（提示拆分） |
| G10 | 技术回归与独立业务题分层；指定关键指标冷 CLI/模型问答、语义和对账全部通过 | 失败阻止对应业务交付；不得用总体分数或管理员强制发布保留认证 |
| G11 | 指标语义差异：聚合、过滤（含码值类别）、粒度、时间维度、默认锚点的变化都已由 owner 审核；受影响的示例与评测题已列出 | 阻止 |
| G12 | 名称生命周期：删除的名称已弃用足够版本，且没有认证指标 / 已验证示例 / 评测题仍在引用 | 阻止 |
| G13 | 列分类：所有将被授权的列都已完成分类（`auto_accepted` 或 `confirmed`）；分类为 `personal` / `sensitive` 的列只授予显式允许的角色 | 阻止 |
| G14 | 选中表/列观测完整；候选到 effective/Pack 有覆盖账本；类型元数据保真 | 不完整阻止依赖构建，旧 facts 不能伪补字段 |
| G15 | 数据覆盖、源对账、时效与一致性满足该业务声明；source_time 未测量不升级 | 不满足阻止相应完整/最新指标声明；允许另行定义当前可见快照统计 |

回滚先校验旧 Pack 与当前 schema、字典/场景资产、权限和数据代际兼容。只在绑定兼容时回退指针，不恢复已撤销权限；否则停止受影响指标或执行获批迁移。不能把单文件/tag 回退当成整个数据与语义链路的回滚。

## 7. 编译：Knowledge Pack

### 7.1 按角色编译

对每个 `oo_r_<role>` 分别编译一个 Knowledge Pack，编译器执行裁剪：

- 移除该角色无权访问的 schema、表、列（及引用它们的关系、指标、示例）；
- 对有行级过滤的表：移除 `sample_values` 与 `most_common_vals` 类样本（它们可能来自该角色看不到的行）；
- `pii != none` 的列只保留说明，不保留样本；
- 术语中 `maps_to` 指向不可见对象的条目移除；只编译 `status: approved` 的术语；
- 码表文件列出状态、类别和适用范围，未知值明确写“不得据此认证完整指标”；只有角色真实可见且验证通过的字典/recipe 才生成 JOIN 指引，不默认引用 `oo_dict.enum_values`；
- `deprecated` 的名称保留并注明替代项；
- 未通过对账或必要码义未确认的指标，在 `metrics.md` 中显示未就绪及具体缺口，不保留完整认证；已解释类别的小计使用不同名称和范围。

公开标准 SQL 类型从受信类型模型投影，不应被全局隐藏裸词扫描删除；实际对象引用按身份做权限闭包，自由文本与样本仍需保密处理。隐藏列 `time` 不得清空可见列的 `timestamp without time zone`。自定义私有类型名不是通用公共词，仍需安全投影。

2026-10-21 实施补记：`role_pack.rs` 已用封闭公共类型词表修复同名隐藏词误伤，
未知/自定义类型显示 unavailable；平台 schema 不再豁免静态可见性检查，
enum 生成器不再无条件推荐字典 JOIN。已完成合成回归与保留产物的离线核验，
尚未完成 Gateway 运行闭包或发布。具体测试及失败边界见[第一批进展](../../docs/semantic-query-chain-p1-progress.md)。

Pack 缓存绑定 ontology/编译器版本、角色策略摘要、依赖与能力契约；每次使用还需核验当前 catalog/导入代际，缓存命中不代表运行兼容。正式字段及协议变更须另行实现，不把此目标键假装成现有实现。会话按角色取得一致版本，撤权和不兼容漂移不能被旧缓存绕过。

### 7.2 目录结构（沙盒内 `/knowledge/`）

```text
/knowledge/
  README.md              # 从这里开始：版本号、库的用途、领域地图、推荐的表、如何使用 oo、注意事项（≤ 2k tokens）
  INDEX.md               # 所有可见表的一行摘要，按领域分组，便于 grep
  domains/<domain>.md    # 领域说明：实体、核心表、常用 JOIN 路径、相关指标
  tables/<schema>.<table>.md   # 每表一个文件：粒度、主键、列说明、码表引用、关系、注意事项
  metrics.md             # 已认证指标：口径说明 + 规范 SQL + 默认锚点 + 对账状态
  metrics/availability.json # 角色实际指标清单；显式候选与认证指标分列，不证明业务验收
  calendar.md            # 时间锚点与常见说法（“上季度”“近 30 天”）的对应，oo_dict.calendar 的用法
  glossary.md            # 已批准术语
  entities/<entity>.md   # 实体、成员表、如何用 oo resolve 找到 entity_key
  enums/<schema>.<table>.<column>.md
  examples/<domain>.sql  # 已验证示例：每条以注释写问题，便于 rg 检索
  relationships.md       # JOIN 路径表，含基数与推荐 JOIN 方式
  manifest.json          # 以上全部内容的机器可读版本（给 SDK/MCP 用）
```

一个表文件示例（节选）：

以下模拟已安装并授权 `logistics`、`oo_dict`、`oo_entity` 及 resolve 能力的目标环境；未满足前提时编译器不得输出这些操作指令。

```markdown
# erp_mysql.kh — 客户主档（ERP 原始表）

> 原始副本。分析请优先使用 `logistics.customer`。粒度：每个客户账号一行。主键：`khid`。约 18.4 万行。

| 列 | 含义 | 类型 | 说明 |
|---|---|---|---|
| khid | 客户 ID | bigint | 主键 |
| khmc | 客户名称 | text | 同一客户可能有多种写法：先用 `oo resolve customer "<名称>"` 得到 `entity_key`，再经 `oo_entity.customer_members` 关联 |
| zt | 客户状态 | text | 码表见 enums/erp_mysql.kh.zt.md：1 正常 / 2 冻结 / 9 停用；另有码 7 含义未确认。按状态过滤请 JOIN `oo_dict.e_erp_mysql__kh__zt` 并按 `class` 选取 |
| xyed | 信用额度（元，含税） | numeric | 0 表示未授信，不是无限额度 |

## 关系
- `erp_mysql.ydh.khid → khid`（多对一，推荐 LEFT JOIN）

## 注意
- 2019 年前存在重复客户，已在 logistics.customer 中合并。
```

### 7.3 渐进披露的阅读路径

`AGENTS.md`（见 [09](09-sandbox-and-agent-runtime.md)）规定的阅读顺序：

1. `README.md`（总是）→ 2. `INDEX.md` 或 `rg` 定位候选表 → 3. 对应 `tables/*.md`、`metrics.md` → 4. 需要时看 `examples/*.sql` → 5. 写 SQL。

沙盒外的 Agent 通过 `read_knowledge(path)`、`describe_table`、`search_knowledge(query)` 访问同一个 Pack（见 [08](08-agent-interface.md)），语义完全一致。

### 7.4 导出

编译器插件化，第一版提供 `knowledge-pack`（Markdown + JSON）；后续提供 `osi`、`dbt-semantic`、`cube`、`snowflake-semantic-view`、`wren-mdl` 导出，方便企业把审核过的语义资产带到其他系统。

## 8. 版本与一致性语义

| 场景 | 行为 |
|---|---|
| 会话进行中发布了新版本 | 会话继续使用开始时的版本；会话记录中写明版本 |
| 发布后表结构变化（加列） | 新列同步数据但不授权（I-PL-8）；进入列分类审核；分类完成并在下一个版本发布后才对 Agent 可见。在此之前 `_oo.table_quality.hidden_columns > 0`，`describe_table` 注明“有 N 列待分类，暂不可用” |
| 发布后表被暂停（删列/改类型） | Gateway 对该表返回 `TABLE_PAUSED`；`get_freshness` 显示 `paused`；Agent 按 AGENTS.md 如实告知 |
| 码表出现新值 | 已部署的巡检记录 unexplained、质量状态与审核项；撤销依赖未知含义的完整指标就绪，不仅静默排除；未部署巡检不能承诺即时发现 |
| 管家合并/拆开实体、修改别名 | 数分钟内物化到 `oo_entity`，不需要发布；审计可追溯 |
| 码值含义被纠正 | 以新的有效期区间记录；按时点的查询使用当时有效的含义，已导出的结果不会被改写 |
| 财务结账 | 在控制台更新 `last_closed_period_end`，立即生效 |
| 回滚 | 核验 schema、派生资产、权限和代际兼容后切换；不恢复已撤销授权，不兼容则停用相关指标 |

## 9. 质量度量

Ontology 质量不看“写了多少”，看对 Agent 的效果与可证明的准确度，具体方法见 [13 评测](13-evaluation-and-benchmarks.md) 轨道 D：

- **覆盖率：** 可见表/列中有已审核描述的比例；码表覆盖率（按已批准含义覆盖的行占比）；实体有解析配置的比例；待分类列数与平均等待时长。
- **实体解析：** 自动合并的精确率（抽样人工复核，目标接近 100%，宁可漏合并不可错合并）；管家队列积压与处理时长；拆开操作的次数（错误合并的信号）；不变量通过率。
- **口径：** 认证指标的对账通过率；语义变更次数。
- **准确率：** 抽样由领域专家盲评自动描述；关系推断对照人工金标的 precision/recall。
- **效用：** 同一题库在“无 Ontology / 仅结构 / 自动草稿 / 审核后”四种条件下的正确率差异（消融）。
- **成本：** 构建一次的 LLM token 与耗时；每百张表的人工审核时长。

### ADR（BLD-1）：角色 Pack 的安装上限与请求预算分离

状态：部分实现；不是整库 Self-build 已交付的声明。

一个 library project 应覆盖全部成员来源的表及逐表 generation 向量；
S0–S4 在全库范围先生成确定性事实，16 表限制仅用于 S5 起草批次。
不得把传输分片当成多个面向 Agent 的 Pack。确定性事实、人工 overlay
与 LLM 草稿的优先级仍遵循 §3.1，不能用提高预算绕过审核或权限闭包。

`oo-builder` 增加 `compile_role_pack_with_ceilings`，第三个参数
`RolePackCeilings` 来自可信安装侧，而不是 LLM 或 visibility 请求。
配置同时覆盖数据集、有效列、可见列、关系、指标、术语、示例、码表、
码值、实体及文件/总字节/文件数上限。所有上限必须为正，总字节必须
能容纳一个文件，有效列预算不能小于可见列预算。请求中的 `limits`
仍独立执行，且不得超过安装上限；扩大安装上限不会授予任何可见性。

兼容入口 `compile_role_pack` 保持原来的默认预算与产物协议：64 个
数据集、每表 1024 个有效列/256 个可见列、单文件 262144 字节、
总计 1048576 字节、128 个文件。大型安装可显式提供例如
`max_datasets=4096`、`max_total_bytes=67108864`、`max_entries=16384`，
其余字段按实际容量审定。不可将 0 解释成无限制。

限制与升级条件：此接口尚未接到 Self-build 服务的安装配置；CLI
仍走兼容入口。JSON 输入扫描仍受 `oo-core` 的 1 MiB/50000 节点
预算约束，因此只扩大 Pack 输出预算不足以证明 600 表宽库可用。
全库事实采集、跨批上下文、合并、域地图以及安装配置贯通需另行实现
并经多来源宽表夹具与真实聚合质量报告验证。当前回归只证明一个
129 表匿名 effective OOS 能在显式预算下编译成一份 Pack，同时低于
实际表数的安装上限和非法上限仍 fail closed。

#### B1 安装配置贯通（追加）

Self-build 安装配置支持 `budgets`：`max_tables` 默认 4096，
`max_document_bytes` 默认 67108864，`max_json_nodes` 默认 4000000，
`role_pack` 为 `RolePackCeilings`（遗漏字段采用兼容默认）。原生维护
Pack 编译使用此可信配置，sealed collection 加载使用同一表数与扫描
预算。`pg-builder` 配置的 `project_max_tables` 默认 4096；
`budget.max_tables` 仍是最多 16 表的 S5 批预算，二者不互相替代。
`oo-core` 的 `JsonScanLimits` 与 `SealedImportLimits` 为显式预算入口，
兼容调用保留旧默认；重复键、深度、节点、表数、逐表列数、receipt
binding vector 和摘要检查仍执行。安装方需同时配置输入与输出预算。

#### B2 全库事实入口与证据边界

`tools/pg-builder/scripts/library_selfbuild.py --config <安装配置> --output <新目录>`
接受安装侧 `psql_command`、`ddl_directory`、`max_tables`、
`statement_timeout_ms` 与可选的 LIB `library_manifest`（`oo.library.manifest/v1`）。
manifest 的 inventory 扣除带原因的 exclusions 必须被精确覆盖；每表保留
source_id/source_generation/target_generation，不合成单一来源 generation。
SHOW CREATE TABLE 文件是离线 S0 元数据输入，不连接源库；解析列注释、
主键和 DDL 摘要，按实际物理映射查询 Agent DB，仅执行元数据或聚合 SELECT。
每次查询均处于显式 READ ONLY 事务，设 statement_timeout 并回滚。

输出 `s0-s2.json`、`facts.json` 与聚合 `report.json`。画像优先 pg_stats，
低基数语义列补精确 distinct；候选关系跨整个 inventory，信号包括键名、
角色前缀、注释和类型。验证同时保留 distinct-value containment 与行加权
containment；少量孤立码值不能再掩盖大部分行上的可靠关系。主键目标必须
实际唯一，空表、超时与预算未测量均记录，不冒充通过。`max_relationship_checks`
以安装配置和当前实现为准，可按容量显式扩大；预算未测量候选保留为诊断证据，但完整性门禁拒绝将其用于成功构建，不得借扩大预算自动批准关系。
S4 按字典类型分组验证代码集合，保留歧义候选；候选含义不自动批准。

质量报告须区分候选召回、强证据召回、已裁定 precision、未裁定数量与
保守 precision 下界。有限正例金标不能证明全库 precision；数据中全为
空值/占位值的已知 schema 关系仍是候选，不能伪造包含度。所有事实与
报告均应保存私有 evidence，不把客户名称、列名或取值写入 Git。

#### B3 有界草稿与确定性事实优先

同一库 facts 按至多 16 表切分 S5 卡片，每批包含触及本批的全局候选
关系、码表和邻表主键摘要。显式 `draft_datasets` 与 `max_draft_batches`
控制实际起草范围；默认不调用模型，整库 S5 必须由操作者明确选择。
请求去除物理映射，不发送样本；结构化草稿只能补描述、label、grain
与术语候选，不能覆盖类型、关系或分类。冲突记录事实摘要并进入审核队列。

每批 completion 预算按列数增长，最高 40000；总预算默认 140000，
请求 deadline 600 秒，并发为 1。Claude 路由不发送 temperature；仅接受
完整 JSON（允许完整 JSON 围栏），finish_reason 必须为 stop，表/列必须
精确绑定。失败不自动紧循环重试，网络失败等待 65 秒；持久失败收据还
记录 retry_not_before。缓存绑定 facts、批次与模型，未完成响应不入缓存。
此入口为安装侧 Builder 命令，不声明已接入控制 API 的异步 stage 调度。

#### B4 一份 effective OOS 与一份角色 Pack

配置 `builder_binary` 与可信的 `pack_ceilings_file` 后，同一命令调用已有
Rust Pack 编译器的 `library-role-pack <安装上限文件>` 入口。旧 `role-pack`
入口及其默认预算不变；扩大输入仍经过重复键、节点、深度与 OOS 校验。
输出包含 effective-oos、visibility、review 和一份 Pack，不创建多个角色
或 Gateway，不执行发布。`fixture_review` 是显式测试模式，所有相关审核
身份为 `fixture/test`，收据始终 `business_owner_signed=false`；不得用于
冒充业务 owner 签字。未裁定候选关系仅在显式
`include_candidate_relationships` 配置下以“未审核”进入测试 Pack。

手机/电话/证件/银行卡/详细地址列默认隐藏，未知文本分类按 restricted
保守隐藏；网点名称、类型、区域、省市等确定性非敏感事实可依产品配置
进入分类候选，业务确认和角色授权独立处理。money 缺少可信单位时不认证口径，进入单位审核项。实体不能仅从可见
键与名称直接认定为业务统一实体；当前交付 effective 的 entities/metrics/examples 均为空，候选进队列不等于审核投影已完成。未经当前角色 Gateway 验证及独立语义验收不编译 verified 示例。
术语继续使用既有 glossary status=approved 与 maps_to 权限闭包；未审核
术语不冒充已批准。空字符串等 OOS 暂不能表达的码值保留于 facts 和审核
项，不改写为虚构码值；可表达的未知码值保留 unexplained 状态与聚合行数。

README 添加有限域地图（最多 16 个前缀组，更多组引导到 INDEX），不枚举
整库表；INDEX 返回每表路径与可见关联表，表页保留现有关系导航。域摘要
目前是确定性的 schema/前缀组说明，不冒充经过业务 owner 批准的领域描述。

#### B5 验证入口与剩余集成边界

匿名回归见 [Library Self-build 操作说明](../tools/pg-builder/LIBRARY-SELFBUILD.md)：
实际独立 PostgreSQL 夹具覆盖 72 表、6 来源、跨批关系、码表、502 列宽表，
5 个 fixture-provider 草稿批次合并到单 Pack，并验证重放不新增模型调用。
fixture provider 不冒充真实模型。真实模型小批必须另保留模型/事实/响应
摘要与产品编译收据；后续确定性规则改进导致 facts 改变时，测试重基须
显式记录旧/新摘要，不能暗中复用为同一输入的缓存命中。

S4 的确定性回补包括：把空字符串从包含度分母中剔除但保留审核证据；
将注释中的枚举 label 与字典名称做精确聚合匹配；保留部分集合重合及多
个类型候选，并标示低置信。不能因为“有候选绑定”就宣称码义已批准。
过时注释与观察码不一致、空码无法由当前 OOS Label 表达等进入审核队列。

Self-build 服务安装默认 Pack 容量为 4096 数据集、64 MiB 总字节、16384
文件；兼容编译器入口默认不变。原生 S0 prepare 使用安装侧的表数、JSON
字节与节点预算，不能在下游又偷偷退回 16/32 表。`pg-builder` 的 16 表
约束仍是批预算，不是安装项目的 inventory 预算。

剩余边界：本轮库级入口尚未接入控制 API 的异步 S0–S10 worker；不声称
已完成控制台 project 创建到角色发布的端到端集成。大库的关系验证预算、
未测量画像和有限金标会限制可声明的 recall/precision。README 的域说明
目前按前缀确定性分组，不等于业务 owner 审定的域知识；Gateway 搜索实现
不在本轮范围，改进的是 Pack 的 INDEX 路径与关联表入口。原子发布与
真实 Gateway 评测分别由 LIB/EVAL 的流程承接，不在此入口伪造通过。

S0 同时保留 `source_type`（DDL）与 `data_type`（实际 PostgreSQL catalog），
避免把源 `datetime` 当作 Agent SQL 类型。S5 的上下文过密时继续细分批次，
不截断列事实；`max_batch_context_bytes` 默认 240000。关系验证预算未覆盖
的条目保留在诊断候选层，不因“未测量”被当成已否定，也不自动批准；新完整观测门禁下不能作为成功 effective 构建的输入。


## 库级控制面整合（INT）

库级项目、全局 facts/分批 S5、逐对象审核、一个角色 Pack、库级原子发布和单 Gateway
接入见 [ADR-030](adr/ADR-030-library-control-worker-and-gateway.md)。库级安装预算不授予
数据权限；Gateway 保留只读登录、当前导入向量/库 head/fence 校验和原 SQL 子集。
健康库每表给出 target_observations，但源时间/共同源位点未测量仍为 unmeasured，
不把 all-in-one 主库称为只读副本。真实库覆盖及评测以独立执行证据为准。
