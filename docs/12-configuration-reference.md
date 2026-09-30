# 12 配置参考

> Status: Proposed（字段在实现时以 `oo-core` 导出的 JSON Schema 为准）
> Owner: Open Ontology Engineering
> Last reviewed: 2026-10-01

## 1. 配置仓库结构

```text
config/                         # 内置 Git 仓库或企业 Git 仓库
  oo.yaml                       # 部署级设置
  sources/*.yaml                # 数据源（每文件可含多个）
  files/*.yaml                  # 文件上传集
  roles.yaml                    # 角色、绑定、限额
  agent.yaml                    # harness、模型、预算、使用方
  hooks/                        # hook 代码与 SQL
    on_batch/<name>/Cargo.toml  # Rust hook 源码，仅 CI 审核构建，不由生产在线编译
    on_batch/<name>/src/*.rs
    plugins/*.yaml             # 不可变制品与能力清单
    after_load/<scene>/*.sql
  ontology/                     # 见 06 §3.1（含 calendar.yaml 与 entities/*.resolution.yaml）
  eval/
    suites/*.yaml               # 评测集定义
```

所有文件经 `oo validate` 校验；`oo plan` 展示差异；`oo apply` 执行。控制台的编辑最终都落为对这些文件的提交。

## 2. `oo.yaml`

```yaml
deployment:
  name: acme-prod
  timezone: Asia/Shanghai
  locale: zh-CN
secrets:
  backend: builtin              # builtin | openbao | vault | aws-kms | gcp-kms | azure-kv | infisical
object_store:
  backend: local                # local | s3
  s3: { endpoint: https://s3.internal, bucket: oo-prod, region: cn-north-1 }
buffer:
  backend: valkey-streams       # valkey-streams | kafka
agentdb:
  primary: { host: agentdb-primary, port: 5432 }
  replicas: [{ host: agentdb-replica, port: 5432 }]
quality:
  reconcile_max_age: 3d          # 超过则 RECONCILE_STALE
  rejects_alert: { per_table_daily: 1000, ratio: 0.001 }
retention:
  audit_days: 180
  transcripts_days: 180
  outputs_days: 30
  uploads_days: 180
llm_for_builder:                # Ontology Builder 使用的模型（与 Agent 使用的模型分开）
  provider: openai-compatible
  base_url: https://llm.internal/v1
  model: <model-id>
  credential: secret://builder_llm
```

## 3. `sources/*.yaml`

| 字段 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `connector` | string | — | `mysql` \| `postgres` \| 插件名 |
| `connection` | object | — | `host`、`port`、`database`、`tls` 等（连接器定义） |
| `read_from` | `primary`\|`replica` | `replica` | 登记为 replica 却连到主库时警告 |
| `credential` | `secret://…` | — | 不允许明文 |
| `timezone` | IANA 时区 | 部署时区 | 解释无时区时间 |
| `tables.include` / `exclude` | glob 列表 | `["*"]` / `[]` | — |
| `sync` | `auto`\|`cdc`\|`incremental`\|`full` | `auto` | 可按表覆盖 |
| `snapshot` | `oo_chunked`\|`debezium_incremental` | `oo_chunked` | 见 [04 §4](04-pipeline.md) |
| `consistency` | `transactional`\|`per_table` | `transactional` | 跨表一致性模式，见 [04 §5.2](04-pipeline.md)；改为 `per_table` 需管理员确认 |
| `max_txn_events` | int | `200000` | 超过则按超大事务分段写入并带质量警告 |
| `max_snapshot_txn_seconds` | duration | `300s` | 批量路径一致性快照事务的时长上限 |
| `freshness_expectation` | duration | `10m`（CDC）/ `2h`（批量） | 超过时 Gateway 返回 `STALE_DATA` 警告；可按表覆盖 |
| `incremental` | `{cursor, every, overlap}` | `updated_at, 1h, 5m` | 仅批量路径 |
| `limits` | `{max_connections, max_rows_per_sec, window}` | `4, 50000, null` | `window` 例如 `"20:00-08:00"` 限制全量时段 |
| `type_policy` | object | 见 [03 §6](03-sources-and-ingestion.md) | `zero_date`、`tinyint1`、`strip_nul`、`charset_override`；所有替换都写拒绝日志（[04 §5.6](04-pipeline.md)） |
| `overrides.<table>` | object | — | 表级覆盖：`sync`、`every`、`columns.exclude`、`key`、`partition` |
| `sample_values` | `none`\|`masked`\|`raw` | `masked` | 发给 Builder 模型的样本值策略 |
| `hooks` | 列表 | `[]` | `on_batch` hook 名 |
| `allow_write_capable_credential` | bool | `false` | 危险：显式允许有写权限的账号（记录审计） |

## 4. `files/*.yaml`

| 字段 | 说明 |
|---|---|
| `schema` | 目标 schema，必须以 `files_` 开头 |
| `mode` | `replace` \| `append` \| `upsert` |
| `key` | `upsert` 时的键 |
| `parse_profile` | 解析配置（表头行、列名、类型覆盖、忽略行），由预览页生成 |
| `sheets` | Excel：要导入的 sheet 与目标表名映射 |

<a id="roles"></a>

## 5. `roles.yaml`

```yaml
defaults:
  limits: { max_rows: 200, max_rows_cap: 2000, export_rows: 500000, export_bytes: 1GB,
            concurrency: 2, statement_timeout: 30s, export_timeout: 120s }
  allow_external_clients: false

roles:
  analyst_all:
    label: 全局分析
    grants:
      - schema: logistics
        tables: ["*"]
      - schema: erp_mysql
        tables: ["*"]
        exclude: ["erp_mysql.sys_user*"]
    column_masks:
      logistics.customer.phone: mask_phone          # 通过掩码视图实现
    pii_access: [personal]                          # 可见的个人信息等级；默认 []，即 pii != none 的列都不可见
    allow_external_clients: true

  dispatcher_eu:
    label: 欧洲区调度
    grants:
      - schema: logistics
        tables: ["shipment", "route", "customer"]
      - table: logistics.customer
        columns: { exclude: [phone, credit_limit] }
    row_filters:
      logistics.shipment: "region = ANY (_oo.oo_claim_array('region'))"
      logistics.customer: "region = ANY (_oo.oo_claim_array('region'))"
    limits: { max_rows: 500, concurrency: 1 }

bindings:                        # 自上而下，第一条匹配生效
  - match: { idp_group: "data-team" }
    role: analyst_all
  - match: { idp_group: "eu-dispatch" }
    role: dispatcher_eu
    claims:
      region: ["{{ user.attributes.region }}"]
  - match: { any: true }
    role: null                   # 未匹配的用户不能使用
```

`tables: ["*"]` 与未写 `columns` 的授权**不会**编译成表级 `GRANT`，而是在每次发布时展开为“已完成分类、且 `pii` 等级在该角色 `pii_access` 之内”的列清单，逐列授权（[05 I-DB-5](05-agent-db.md)）。源库新增的列在分类并发布之前不属于任何角色。`oo_dict` 中码表视图的授权跟随被解释的列；`oo_entity` 的表按普通对象授权，需要时声明行级过滤。

校验规则（`oo validate`）：

- `row_filters` 的表必须在该角色的 `grants` 中；`oo_entity` 的表若包含该角色受行级过滤约束的实体，也必须声明过滤；过滤表达式用 Rust `pg_query` 解析，只允许引用该表的列、`_oo.oo_claim_array()` / `_oo.oo_claim()`、常量与白名单函数；
- 源表有行级过滤时，所有从它派生的场景表必须声明过滤（否则报错）；
- 业务场景视图必须 `security_invoker = true`；平台日历仅允许 [05 §6.3](05-agent-db.md) 的受限所有者例外；
- 每个角色编译出的 Knowledge Pack 不超过预算。

## 6. `agent.yaml`

```yaml
harness:
  default: pi
  versions: { pi: "<pinned>" }
model:
  provider: anthropic            # anthropic | openai | openai-compatible | …
  model: <model-id>
  endpoint_alias: anthropic      # 出口代理上的路径 /llm/anthropic
  credential: secret://agent_llm
  fallback: { provider: openai-compatible, model: <model-id>, endpoint_alias: internal }
budgets:
  per_session_tokens: 2000000
  per_turn_tool_calls: 60
  per_turn_seconds: 600
session:
  idle_timeout: 30m
  max_lifetime: 8h
  workspace_snapshot: false
sandbox:
  driver: docker-gvisor
  image: ghcr.io/<org>/oo-sandbox:<version>
  resources: { cpu: 2, memory: 4Gi, disk: 5Gi, pids: 512 }
  pool: { min_idle: 2, max_pool: 20, max_idle_age: 1h }
instructions_append: |
  本公司财年从 4 月开始。金额默认单位为元。
channels:
  web: { enabled: true }
  feishu:
    enabled: true
    app_id: cli_xxx
    credential: secret://feishu_app
    group_policy: dm_details     # dm_details | full_in_group（需管理员声明群成员权限一致）
  slack: { enabled: false }
```

## 7. `hooks`

```yaml
# hooks/hooks.yaml
hooks:
  mask_phone:
    type: on_batch
    builtin: mask
    params: { columns: ["*.lxdh", "*.phone"], pattern: "keep_first3_last4" }
  scene_logistics:
    type: after_load
    triggers: [erp_mysql.kh, erp_mysql.ydh, tms_pg.shipment]
    debounce: 10m
    sql_dir: hooks/after_load/logistics/     # 按文件名顺序执行；视图须 security_invoker
    materialize: [customer, customer_alias]  # 其余为视图
```

## 8. 评测集定义

```yaml
# eval/suites/customer-core.yaml
suite: customer-core
visibility: private              # public | private（private 只在客户环境运行，不出库）
snapshot: eval_2026_10_01        # 评测使用的冻结数据快照（见 13 §5）
runs_per_item: 3
scoring: { numeric_rel_tol: 0.001, pass_rule: pass^3 }
items_file: eval/suites/customer-core.items.jsonl
```

题目格式见 [13 §4](13-evaluation-and-benchmarks.md)。
