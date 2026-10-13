# 01 统一准入路线与支持矩阵

> Status: Proposed
> Last reviewed: 2026-10-09
> 依赖：[README](README.md)；对应 Spec：REQ-SRC-08、REQ-SRC-15；AT-SRC-06、AT-SRC-14

## 1. 术语

| 术语 | 定义 |
|---|---|
| **家族 family** | 共享线协议与 catalog 形态的一组产品：`mysql`、`postgres`、`mssql`、`oracle`、`db2`、`hana`、`native-cn`（达梦、神通等自有协议）、`warehouse`、`lake-table`、`olap`、`document`、`kv-stream` |
| **目标 target** | 准入的最小单位：`(家族, 产品, 版本区间, 部署变体, 驱动)`。例：`mysql / Aurora MySQL 3.x / managed / sqlx`、`mysql / MariaDB 11.4 / self-managed / sqlx` |
| **部署变体 variant** | `self-managed`、`rds`、`aurora`、`cloud-sql`、`alloydb`、`azure-sql`、`polardb`、`serverless-cloud` 等；托管变体在日志访问、参数、权限、故障转移上与自建不同，**独立成目标** |
| **能力 capability** | 目标上可独立验收的功能项，见 §3 |
| **等级 level** | 目标 × 能力当前所处的准入阶段，见 §2 |
| **矩阵 matrix** | 所有目标 × 能力 × 等级 × 证据的机器可读真相源，见 §4 |

## 2. 准入等级

```text
candidate ──▶ designed ──▶ experimental ──▶ verified ──▶ certified
                              │                │             │
                              └──── 降级 ◀─────┴─────────────┘
excluded：明确不接入（附原因与替代路径）
```

| 等级 | 进入条件 | 允许的使用 | 对外标识 |
|---|---|---|---|
| `candidate` | 列入候选；无设计 | 无。注册返回 `CONNECTOR_UNSUPPORTED`，提示文件/dump 路径 | “未支持” |
| `designed` | 有该目标的 ADR：能力声明、DBA 清单草案、威胁评审（含写入分类 R0–R3）、驱动与许可证决定、成本模型；Spec/Plan 已登记 | 无运行代码被启用 | “设计中” |
| `experimental` | 连接器实现并默认关闭（feature + 配置 `experimental: true`）；夹具套件（类型、分块、读取、取消、零写）在**至少一个真实实例**上通过；DBA 清单被一名外部 DBA 或客户 DBA 走读 | 仅非生产 profile；控制台显示实验标识；不得被 `sync: auto` 自动选用 | “实验” |
| `verified` | 每个声明的版本、变体都在真实实例上通过完整套件（[05](05-conformance-and-evidence.md)）；源端审计零写；支持/不支持项公开；夜间回归连续 14 天通过；独立 reviewer（数据 + DBA + 安全）签字；证据包 hash 固定 | 生产可用，限已验证的能力与版本 | “已验证（版本、能力列表）” |
| `certified` | `verified` 之上：至少一个真实客户或等价规模环境运行证据；规模测试（A 轨 s100k 与 5m nightly、源读取预算实测）；故障演练（位点过期、故障转移、DDL）；DBA Runbook 经真实演练；连续三个发布无回归 | 生产推荐 | “认证” |
| `excluded` | 评审结论不接入 | 无 | “不支持（原因）” |

规则：

1. **逐能力定级。** 同一目标的 `batch_full` 可以是 `verified`，`cdc` 仍是 `designed`。用户界面和 `sync: auto` 只看已达到 `verified` 的能力。
2. **不得跨级使用。** `experimental` 能力不能由管理员在生产 profile 打开；若客户坚持，需要书面例外并记录在 apply 审计中，仍不改变矩阵。
3. **晋升必须有证据引用。** 矩阵每个 `verified`/`certified` 单元格必须引用证据包 ID、固定提交与套件版本；没有证据的单元格由 CI 拒绝。
4. **不继承。** 新版本、新变体、新驱动、新兼容模式都从 `designed` 起；唯一例外是**补丁级**版本（同一小版本线的 patch），由套件在新 patch 上重跑通过即可沿用，且需记录。
5. **公开的限制是等级的一部分。** `verified` 必须同时发布限制清单（不支持的类型、LOB、DDL、删除语义、位点形态）。

## 3. 能力清单

| 能力 | 含义 | 最低验收内容 |
|---|---|---|
| `catalog` | 读取 schema、列、键、注释、权限体检；生成 AccessReport | AT-SRC-01、AT-SRC-04 |
| `logic` | 读取视图、存储过程/函数、注释（Self-build S0 的证据） | 视图正文与方言标记完整；无权限时 block 而非跳过 |
| `query_log` | 可选：读取查询历史作为 S3/S7 证据 | 需标注许可证/授权要求（如 Oracle AWR 依赖 Diagnostic Pack 许可），未授权时关闭 |
| `batch_full` | 全量只读读取（分块、取消、预算） | AT-SRC-02、AT-SRC-03 |
| `batch_incremental` | 游标/rowversion/分区刷新增量 | AT-PL-03，含游标安全检查（[03 §6](03-change-capture-and-consistency.md)） |
| `change_feed` | 引擎原生变更读取（CT、CHANGES、CDF、快照差） | 保留窗口、过期处理、删除检测 |
| `cdc` | 日志级 CDC | AT-PL-01、AT-PL-04/05，位点序与事务边界 |
| `delete_native` / `delete_reconcile` | 源删除如何被感知：原生，或按主键键集对账 | AT-PL-06；`delete_reconcile` 附带 `DELETES_LAGGED` 披露 |
| `snapshot_token` | 可用于一致读取的位点/版本/时间戳（AS OF、RR 快照、SCN） | 同一 token 跨表一致读取演示；不可比位点禁修复 |
| `replica_read` | 从只读副本读取，并能证明副本回放进度 ≥ 快照位点 | DEC-06 屏障 |
| `budget` | 读取预算可强制（服务端 + 客户端） | AT-SRC-02、AT-SRC-12 |

## 4. 支持矩阵

### 4.1 真相源与呈现

- 机器可读文件：先以 [support-matrix.yaml](support-matrix.yaml) 作初稿；DBX-W01 完成后迁入 `contracts/source/support-matrix.yaml`，随契约版本化。
- **生成而非手写**：`verified`/`certified` 单元格由 `xtask` 从 Evidence 清单渲染；手工只能写 `candidate/designed/excluded`。CI 检查：矩阵每个高等级单元格能解析到真实证据包；证据包 hash 与矩阵一致；矩阵文档与 YAML 无漂移。
- **三处呈现**：
  - `oo sources supported`（CLI）与控制台“添加数据源”向导：只列已达 `verified` 的目标，`experimental` 折叠显示并带限制；
  - 注册时的 AccessReport 把检测到的 `detected_version`、变体与矩阵比对，输出 `target_id`、各能力等级；
  - 文档页由 YAML 渲染的表格（发布时随制品）。

### 4.2 YAML 形态（节选）

```yaml
schema: oo-support-matrix/v1
levels: [candidate, designed, experimental, verified, certified, excluded]
targets:
  - id: sqlserver-2022-selfmanaged
    family: mssql
    product: SQL Server
    versions: ["16.x"]
    variant: self-managed
    driver: { kind: native-rust, crate: tiberius }
    wave: 1
    grade: { cdc: T, change_feed: S1, batch_incremental: B }
    caps:
      catalog: designed
      batch_full: designed
      change_feed: designed        # Change Tracking
      cdc: designed                # 内置 CDC 表 + Debezium 读取
      delete_native: designed
    limits: ["datetime2(7) 精度截断为微秒", "CDC 需 DBA 启用并在 Standard 2016 SP1+ 可用"]
    evidence: []                   # verified 起必填
```

### 4.3 错误码与行为

| 场景 | 错误码 | 行为 |
|---|---|---|
| 目标家族/产品不在矩阵 | `CONNECTOR_UNSUPPORTED` | 注册失败；提示文件/dump 或审核外部连接器路径 |
| 家族已知但检测到的版本/变体未验证 | `TARGET_VERSION_UNVERIFIED` | 默认拒绝；`experimental` 目标需显式开启并显示限制；不静默沿用相邻版本结论 |
| 请求的能力（如 `sync: cdc`）该目标未 verified | `CAPABILITY_UNVERIFIED` | 显式 `sync: cdc` 报错，不降级；`sync: auto` 只在已验证能力中选择 |
| 兼容模式未知（如 DM 初始化兼容参数、Kingbase 模式） | `COMPAT_MODE_UNKNOWN` | block；AccessReport 要求运维确认模式 |
| 驱动缺失（客户未提供专有驱动） | `DRIVER_MISSING` | block；给出驱动 digest 与放置说明；不回退到通用驱动 |

## 5. 降级与退役

| 触发 | 动作 |
|---|---|
| 夜间/发布前套件在 `verified` 目标上失败 | 连续两次失败 → 矩阵自动降为 `experimental`，发布说明列出；已注册源继续同步但控制台显示警告；新注册默认拒绝 |
| 安全问题（零写审计失败、权限扩张、凭证泄漏） | 立即 `blocked`：新注册与已有源的**该能力**暂停；已有数据只读保留，质量状态 `SOURCE_CAPABILITY_BLOCKED` |
| 厂商 EOL | 到 EOL 后版本从“支持”降到 `experimental`，满 2 个发布后移除；移除前提示迁移 |
| 驱动 CVE/许可变更 | 按严重度走供应链流程；无法修复则同上降级 |
| 上游行为变化（Debezium、云服务 API） | 套件在固定镜像 digest/API 版本上运行；升级需重新通过后才更新矩阵 |

降级不得**静默停止**用户已有的同步：状态变化必须出现在新鲜度、控制台与审计里（沿用现有质量警告通道）。

## 6. 托管、兼容与客户私有目标

- **托管变体与自建分别成目标**，不做“RDS = 自建 + 参数”的推导。证据中必须包含该变体的真实实例或等价环境。
- **兼容层独立成目标**：TiDB、OceanBase、PolarDB-X、GoldenDB、TDSQL、openGauss/GaussDB、Kingbase、HighGo 各自有目标，不继承 MySQL 或 PG 家族的结论；可以复用家族的**实现代码**，但验收单元格独立。
- **客户私有目标**：某客户环境中的某个库只在该客户部署上验证时，矩阵条目标 `scope: deployment-local`，只在该部署的矩阵中出现，不进入公开发行矩阵；其证据保留在客户环境（沿用私有证据边界）。

## 7. 许可与合规闸门

进入 `experimental` 之前必须完成（逐目标记录）：

1. 驱动许可：是否可由项目分发；不可分发则采用“客户提供驱动卷 + digest 记录”。专有驱动（如 Oracle Instant Client、Microsoft ODBC Driver、IBM Db2 驱动、SAP HANA 客户端、达梦/神通驱动）的再分发条款**不假定可分发**，由法务逐项确认（部分条款本次检索未核实，见 [06](06-risks-open-questions-and-sources.md)）。
2. 测试夹具许可：开发版/免费版是否允许用于自动化 CI；需要商业许可的版本只在客户或受保护环境验证。
3. 厂商条款中对性能基准发布、反向工程、云 API 调用频率的限制，由法务确认后才能公开对应证据。
4. 依赖审计：native 库、JVM 组件进入供应链清单（SBOM/NOTICE），并经 `cargo-deny`/镜像扫描。

## 8. 波次（优先级，不是日期承诺）

| 波次 | 目标 | 说明 |
|---|---|---|
| 0（进行中） | PostgreSQL 13+ 自建、MySQL 8.x 自建、文件/SQLite/dump | 现行 Spec 范围；当前**没有任何连接器级 verified 声明**，仅有若干限定切片被批准 |
| 1 | PG 托管变体（RDS、Aurora、Cloud SQL、AlloyDB）；MySQL 托管变体（RDS、Aurora、Cloud SQL、PolarDB MySQL）；MariaDB 11.4 LTS 起；SQL Server 2019/2022（含 Azure SQL MI、RDS） | 同家族代码复用度最高，主要新增体检与证据 |
| 2 | Oracle 19c/23ai（批量优先）；Db2 LUW 11.5（批量）；SAP HANA 2.0（批量）；国产库：OceanBase MySQL 模式、TiDB、openGauss/GaussDB、Kingbase、HighGo、GoldenDB、TDSQL（先批量）；达梦、神通（外部驱动 worker，实验） | 许可与驱动最复杂，先批量，CDC 逐目标另评 |
| 3 | Snowflake、BigQuery、Databricks（Delta）、Redshift；ClickHouse、StarRocks、Doris；Iceberg 表；Trino（受限） | 以 change_feed / 分区刷新 / 快照为主；成本预算是核心 |
| 4 | MongoDB、DynamoDB、Elasticsearch/OpenSearch、Cassandra/ScyllaDB；Kafka 主题（经 buffer 适配器） | 半结构化映射与无事务语义 |
| 不接入 | Redis/Valkey 作为源（keyspace 通知有损，无持久变更日志）；SAP HANA 触发器复制；Db2 ASN 由平台安装；任何需平台在源库创建对象/写表的方案 | 见 [03 §1](03-change-capture-and-consistency.md) |

每个波次内部逐目标独立推进，达不到证据要求的目标不进入矩阵的 `verified`。
