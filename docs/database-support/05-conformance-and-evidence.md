# 05 连接器一致性套件、夹具与证据

> Status: Proposed
> Last reviewed: 2026-10-09
> 依赖：[01](01-admission-and-support-matrix.md)、[03](03-change-capture-and-consistency.md)；Spec 第 18 章、20 章；Plan §5、§6
> 对应 Spec：REQ-SRC-15；AT-SRC-06 至 AT-SRC-14

## 1. 套件：声明什么就测什么，不声明就必须拒绝

套件是黑盒的**协议级**测试，对任意目标运行同一组用例；每个用例由目标声明的 `Capability` 触发。两条总规则：

- **声明必有测试**：目标的 `capabilities()` 中每一项都必须映射到至少一个套件用例，且用例在该目标上通过；没有测试的声明由 CI 拒绝（AT-SRC-14）。
- **未声明必拒绝**：未声明的能力被请求时返回 `CAPABILITY_UNVERIFIED` 或 `unsupported`，套件有负向用例证明不会静默降级。

| 用例 | 内容 | 适用能力 | 关联 AT |
|---|---|---|---|
| CC-01 声明诚实性 | 能力声明 ↔ 用例映射；未声明能力的拒绝路径 | 全部 | AT-SRC-14 |
| CC-02 源端零写审计 | 在平台身份下运行**启动、体检、读取、重连、no_data、取消、对账**全流程，语句级审计无 DML/DDL（R1w 例外另列） | 全部 | AT-SRC-01、AT-SRC-07 |
| CC-03 类型保真 | 目标全类型边界夹具；CDC/批量/文件同值 hash；金额/大整数/十进制无损；精度截断被声明而非静默 | catalog、batch、cdc | AT-SRC-03、AT-SRC-11 |
| CC-04 分块覆盖 | 稀疏/组合/字符串/负数/长标识符键；无 PK 只 full；**二进制排序**（§3） | batch_full | AT-SRC-04 |
| CC-05 游标与重叠 | 相等时间戳、长事务晚提交、时钟回拨、重叠窗口；`cursor_safety` 分类被正确识别 | batch_incremental | AT-PL-03、AT-SRC-09 |
| CC-06 变更读取与过期 | token 锚定快照 + 变更重放；窗口过期 → `POSITION_EXPIRED` → 重快照；不推进旧位点 | change_feed、cdc | AT-SRC-08 |
| CC-07 删除收敛 | 原生删除；`reconcile` 的可见延迟 ≤ 配置且标 `DELETES_LAGGED`；删除后旧 upsert 不复活 | delete_* | AT-SRC-09、AT-PL-06 |
| CC-08 取消与预算 | 取消在源端终止（会话/云作业）；速率/字节/连接预算被遵守；云计量单位记录；耗尽 → `SOURCE_BUDGET_EXHAUSTED` | budget | AT-SRC-02、AT-SRC-12 |
| CC-09 故障与恢复 | 连接断开、源重启、故障转移、DDL（加列/改类型/截断）、位点回退；暂停并披露，不静默继续 | cdc、change_feed | AT-PL-04/05、AT-SRC-08 |
| CC-10 权限最小化 | 平台账号被检出的写能力、继承角色、`ANY` 类权限、未知权限 → block；DBA 清单生成与回滚 | catalog | AT-SRC-01 |
| CC-11 驱动与供应链 | 驱动 digest、许可记录、缺失 → `DRIVER_MISSING`；不回退通用驱动 | 全部 | AT-SRC-10 |
| CC-12 语义传递 | caveats 从 type_map 经 catalog → Builder → Pack → freshness 全链可见；声明约束 `declared_only` 不被当强 FK | catalog | AT-SRC-11 |
| CC-13 文档映射 | 稳定字段推断、类型漂移、新增字段进入未分类队列、`doc` 列保真 | document | AT-SRC-13 |

## 2. 各引擎的源端写入审计手段

“零写”必须由**源端自己的记录**证明，不是平台自述：

| 引擎 | 审计手段（候选，随目标验证） |
|---|---|
| PostgreSQL | `log_statement='all'` 或 `pgaudit` 的会话审计；`pg_stat_statements` 作旁证 |
| MySQL/MariaDB | 审计插件或 general log（测试实例上）；`performance_schema` 语句事件 |
| SQL Server | SQL Server Audit / Extended Events（按登录名过滤） |
| Oracle | 统一审计（Unified Auditing）按用户策略 |
| Db2 | `db2audit` |
| SAP HANA | 审计策略 |
| Snowflake | `QUERY_HISTORY`（按角色/用户，语句类型） |
| BigQuery | `INFORMATION_SCHEMA.JOBS` 与云审计日志 |
| Databricks | 系统审计表（`system.access.audit`）与 query history |
| Redshift | 审计日志、`SYS_QUERY_HISTORY` |
| ClickHouse | `system.query_log` |
| MongoDB | profiler / 审计日志 |
| DynamoDB | CloudTrail 数据事件 |

审计结果作为证据包的原始文件；解析器与规则版本化，规则变更需 reviewer 审核。

## 3. 分块键排序：`chunk_key_order`

分块与对账依赖“键范围”。源按其排序规则比较字符串，Agent DB 与 Loader 按规范字节比较，二者不一致会造成块重叠或遗漏。规则：

- 字符串/二进制键分块必须强制**二进制排序**：PG `COLLATE "C"`、MySQL `ORDER BY BINARY col` 或二进制排序规则、SQL Server `COLLATE Latin1_General_BIN2`、Oracle `NLSSORT(col,'NLS_SORT=BINARY')`、其他引擎按其二进制排序设施（待各 DBP 验证）。
- 若引擎无法保证二进制排序，或强制排序使索引不可用而严重放大源负载，则该表采用哈希分块（按主键哈希取模）或 `read_keys` 全键扫描，并在 AccessReport 标明。
- 套件夹具必须包含：中文（GBK/GB18030 与 UTF-8）、大小写、重音、尾空格、二进制主键；对不同排序规则的目标分别验证（AT-SRC-04）。

## 4. 夹具与许可

| 目标 | 自动化夹具（候选） | 许可/限制 | 若不可自动化 |
|---|---|---|---|
| PG、MySQL、MariaDB、ClickHouse、MongoDB、Doris、StarRocks | 官方容器镜像，Testcontainers 风格 | 开源许可，可进公开 CI | — |
| SQL Server | 开发者版容器（Linux） | 开发/测试用途；不可用于生产 | 受保护 runner |
| Oracle | Oracle Database Free（23ai）容器 | 免费版有资源与功能限制；19c 需客户/自有许可 | 客户环境证据（`scope: deployment-local`） |
| Db2 LUW | Community Edition 容器 | 资源上限 | 客户环境 |
| SAP HANA | HANA Express | 内存与使用限制，SAP 开发者协议 | 客户环境 |
| 达梦、神通、GoldenDB、TDSQL、金仓、瀚高 | 需向厂商申请开发/试用许可 | 各厂商条款不一 | 客户环境；条款允许时才进 CI |
| OceanBase、TiDB、openGauss | 社区版容器/工具链 | 开源 | 兼容模式差异需逐目标 |
| Snowflake、BigQuery、Redshift、Databricks | **只有真实云账号**；本地仿真器行为不等价，不作 verified 证据 | 费用、配额、凭证 | 受保护环境 + 严格预算；证据类型 `managed_cloud` |
| Aurora、RDS、Cloud SQL、AlloyDB、PolarDB | 真实托管实例；故障转移用例需在托管实例上触发 | 费用 | 同上 |
| DynamoDB | DynamoDB Local 可作夹具，但与云服务（保留、限速）不等价 | — | 云证据补充 |

规则：

- 证据类型 `container`、`managed_cloud`、`customer` 在矩阵中**分别标注**；只有 `container` 证据的托管变体不得声称托管能力 `verified`。
- 云账号凭证只存在受保护环境；PR 不得读取；证据不得包含客户或账号敏感值。
- 公开仓库的 PR 对需要受保护环境的目标只运行离线/容器子集，完整验证在受保护 pipeline。

## 5. 版本矩阵与证据包

- 每个目标的版本区间展开为**具体测试版本集合**：区间两端 + 近期补丁 + 已知行为变化点（例如 MySQL 8.0 与 8.4，PG 13/17，SQL Server 2019/2022）；矩阵只写实际测试过的版本。
- 证据包 `evidence/<run-id>/DBX/<target-id>/manifest.json`：目标版本与变体、驱动 digest、套件版本、各用例原始输出与 hash、源端审计原始文件、环境描述（拓扑、托管服务版本）、命令与退出码、评审记录；`generated` 矩阵单元格引用该 ID。
- 失败不覆盖：新证据用新 run-id，旧失败保留（沿用 Plan §2.6）。

## 6. CI 与回归

| 触发 | 范围 |
|---|---|
| PR | 套件的容器子集（CC-01/03/04/10/11/12）与矩阵一致性检查（YAML 与证据、文档无漂移） |
| 夜间 | 所有 `verified` 容器目标的完整套件；受保护 pipeline 对云目标运行带预算上限的子集 |
| 发布前 | 全部 `verified`/`certified` 目标完整套件；任一失败按 [01 §5](01-admission-and-support-matrix.md) 降级并写入发布说明 |
| 上游变更 | Debezium、驱动、云 API 版本升级必须先通过相关目标套件 |

云目标回归的成本由预算配置约束；超预算则跳过并把单元格标为“未在本周期验证”，**不得继续展示为本周期已验证**。

## 7. 晋升评审清单

晋升到 `verified` 前，独立 reviewer 逐项核对：

1. 能力声明与套件映射完整（CC-01）；
2. 源端审计零写（CC-02），R1/R1w 项与 DBA 清单一致；
3. 类型夹具包含该目标全部已声明类型与已知陷阱（[04](04-database-profiles.md) 对应小节）；
4. 删除、位点、保留窗口、故障转移用例通过；
5. 读取预算与取消在源端可验证；
6. 驱动许可与再分发决定已记录；
7. 限制清单已公开且与证据一致；
8. 数据 owner、DBA、安全三类 reviewer 签字（具体人名未确定时标 pending，不得用角色占位冒充签名）。

晋升到 `certified` 另需客户或等价规模证据与故障演练（[01 §2](01-admission-and-support-matrix.md)）。
