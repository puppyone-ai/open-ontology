# 05 引擎版本矩阵与本地真实库测试方案

> Status: Proposed
> Owner: Open Ontology Engineering
> Last reviewed: 2026-11-04
> 依赖：[database-support/05 一致性套件](../database-support/05-conformance-and-evidence.md)、[support-matrix.yaml](../database-support/support-matrix.yaml)、[02 拉取手册](02-ingestion-playbook.md)

## 1. 本期引擎范围

“支持”按 引擎 × 版本 × 能力 计，等级定义见 [database-support/01](../database-support/01-admission-and-support-matrix.md)。v0.2 的目标：

| 档 | 引擎 | 版本 | v0.2 目标等级 |
|---|---|---|---|
| P0 | MySQL | 5.7、8.0、8.4 LTS、9.x | 全链 `verified`（快照、C1、C5、Self-build、Pack、Gateway 对话） |
| P0 | PostgreSQL | 13、14、15、16、17、18（12 已 EOL，`experimental`） | 全链 `verified` |
| P0 | Oracle | 19c、23ai（Free）；11g XE、21c XE | 19c/23ai 全链 `verified`；11g/21c 快照 + C5 `verified`、C1 `experimental` |
| P1 | MariaDB | 10.6、10.11、11.4、11.8 | 全链 `experimental` → 最新 LTS `verified` |
| P1 | SQL Server | 2019、2022、2025 | 快照 + CT/CDC `experimental` |
| P1 | TiDB | 7.5、8.5 | 快照 + C5 `experimental`（无 MySQL 复制协议，CDC 走 TiCDC，另行评估） |
| P1 | OceanBase CE（MySQL 模式） | 4.x | 快照 + C5 `experimental` |
| P1 | openGauss | 5.x、6.x | 快照 + C5 `experimental` |
| P1 | ClickHouse | 24.x LTS、26.x | 快照 + 分区刷新 `experimental` |
| P1 | Doris | 3.x、4.x | 快照 + 分区刷新 `experimental` |
| P1 | StarRocks | 3.x | 快照 + 分区刷新 `experimental` |
| P1 | MongoDB | 7.0、8.0 | 快照 + change stream `experimental` |
| P1 | OpenSearch / Elasticsearch | OS 2.x/3.x、ES 8.x | 快照 `experimental` |
| P2 | Db2 LUW | 11.5、12.x | 快照 `experimental` |
| P2 | 达梦 DM8、KingbaseES | 官方 tar 镜像 | 快照 `experimental`（依赖官方镜像获取与许可） |
| — | 文件（CSV/Excel/Parquet/JSONL/SQLite/dump） | — | 已有路径，保持 |

合计 16 个数据库引擎（P0 3、P1 10、P2 3）+ 文件。P0 是本期退出条件；P1 在同一日程内并行推进，未达目标的如实留在较低等级；P2 视镜像与许可条件推进，不阻塞退出。Oracle 12c 没有可公开获取的镜像，除非取得官方镜像或真实实例，否则不进矩阵。

## 2. 镜像与运行位置

镜像全部用 digest 锁定，记录在 `testing/infra/images.pins` 与 `images.lock.json`（沿用现有格式）。架构信息为 2026-11-03 查询 registry 所得：

| 引擎/版本 | 镜像 | arm64 原生 | 运行位置 |
|---|---|---|---|
| MySQL 5.7 | `mysql:5.7.44` | 否（仅 amd64） | 本机 amd64 仿真 |
| MySQL 8.0 / 8.4 / 9.x | `mysql:8.0`、`8.4`、`9.x` 具体补丁号 | 是 | 本机原生 |
| PostgreSQL 12–18 | `postgres:<major>.<minor>-bookworm` | 是 | 本机原生 |
| Oracle 23ai Free | `gvenzl/oracle-free:23-slim` | 是（23.5 起） | 本机原生 |
| Oracle 19c EE | `container-registry.oracle.com/database/enterprise:19.x` | 有 ARM 版 | 需 Oracle 账户签入并接受许可后拉取（Plan D2） |
| Oracle 11g / 21c XE | `gvenzl/oracle-xe:11`、`21` | 否 | 本机 amd64 仿真 |
| MariaDB | `mariadb:<ver>` | 是 | 本机原生 |
| SQL Server | `mcr.microsoft.com/mssql/server:2019/2022/2025-<CU>` | 否 | 本机 amd64 仿真（2025 用 CU1 及以后） |
| TiDB | `pingcap/tidb:v<ver>`（固定具体标签，`latest` 已停更） | 是 | 本机原生 |
| OceanBase CE | `oceanbase/oceanbase-ce` | 是 | 本机原生 |
| openGauss | `opengauss/opengauss` | 是 | 本机原生 |
| ClickHouse / MongoDB / OpenSearch / ES | 官方镜像 | 是 | 本机原生 |
| Doris | `apache/doris:fe-<ver>` / `be-<ver>` | 是（4.1.5 起） | 本机原生 |
| StarRocks | `starrocks/allin1-ubuntu` | 是 | 本机原生 |
| Db2 | `icr.io/db2_community/db2` | 否 | 本机 amd64 仿真 |
| 达梦 / KingbaseES | 官方下载 tar 后 `docker load` | 视包而定 | 需人类下载并确认许可 |

**SQL Server 现状（2026-11-04，S3-E-02）。** 2019 CU32、2022 CU20、2025 CU1 在 `oo-test matrix --engine sqlserver` 上通过完整夹具 T0/T1/T2/T8（231 表、2M 事件、300 列宽表，并发写入下 Agent DB 零差异；C5、Change Tracking、CDC 各三周期，空闲周期零写入、零新代际；错误 epoch 拒绝且不改持久状态）与 T7（只读审计归因；CDC 空闲映射追加按 D12 归因）。T3/T4 未做，级别仍由生成矩阵决定。

**OLAP 现状（2026-11-04，S4-F-01）。** ClickHouse v26.8.21.10、Doris 3.1.4、StarRocks 3.5.21 经 MySQL 线协议读取（ClickHouse 端口 9004、`readonly` profile，不引入 HTTP crate），在 `oo-load` 上通过 T0/T1/T2/T8：快照按分区切块（超预算分区按大小档哈希分桶，计划跨周期稳定），分区刷新只重读摘要变化的范围，空闲周期零读写，结构变化拒绝且不改目标；读端连接峰值 1，占空比 ≤25%。夹具规模小（最大表 3 万行），Doris/StarRocks 夹具未启用 TLS，ClickHouse 24.x、Doris 4.x、StarRocks 4.x 未测；新增分区或分区跨档会触发重基线。

**amd64 镜像怎么跑。** 开发机是 Apple Silicon。MySQL 5.7、Oracle XE 11g/21c、SQL Server、Db2 只有 amd64 镜像，本期在本机 Docker Desktop 用 `--platform linux/amd64`（Rosetta）运行，已实测可用（Plan D1）。仿真比原生慢，套件的超时与吞吐预期对这些目标单独放宽并在证据中注明“仿真”。macOS 27 是最后一个完整支持 Rosetta 2 的版本，`oo-test` 因此保留 `--runner remote` 接口，必要时接入一台 x86 Linux 主机，无需改测试本身。

**磁盘。** 全部镜像约需 40–60 GB。`oo-test` 每次运行使用带 `oo.run_id` 标签的容器与卷，结束时只清理自己创建的资源；不清理来历不明的容器或卷。

## 3. 测试夹具

所有引擎共用一份中性合成数据模型（扩展现有 OO-Logistics 生成器，见 [13](../13-evaluation-and-benchmarks.md)），按引擎方言生成 DDL 与数据。覆盖的形状：

| 类别 | 内容 |
|---|---|
| 键形态 | 整数自增主键；`varchar` 主键 + 大小写不敏感排序规则；复合主键；仅唯一索引；无任何键 |
| 规模 | 至少一张 ≥ 2,000,000 行的表（触发多块与自适应）；一张 300 列宽表；数百张小表（模拟上百表 schema） |
| 类型 | 精确小数、无符号整数、时间与时区、LOB/CLOB/BLOB、JSON、枚举/集合、Unicode 与 emoji、空串与 NULL（Oracle 空串即 NULL）、引擎特有类型 |
| 结构 | 声明外键与未声明的隐式关系；分区表；按月分表族（`log_202601` …）；审计/备份表；视图 |
| 语义 | 码表、组织层级、日历、金额与数量指标；敏感列（手机号、身份证样式的合成值） |
| 增长 | 一张在周期间持续增长、越过单块预算的表 |

每个夹具附 20 道金标准问题：答案由源库上的 SQL 计算，存为 gold 文件；题目覆盖计数、跨表关联、分组聚合、时间范围、NULL 处理、码值翻译、多跳实体关系。

## 4. 测试套件

每个 引擎×版本 跑同一套件，结果写成证据包，`verified` 等级只能由证据包生成（[database-support/05](../database-support/05-conformance-and-evidence.md)）。

| 套件 | 内容 | 通过条件 |
|---|---|---|
| T0 预检 | `oo doctor`：版本识别、能力探测、权限、日志条件、分块计划、预算估算 | 识别结果与镜像版本一致；缺权限时输出可执行的 GRANT 清单 |
| T1 全量快照 | 并发写入负载（插入/更新/删除/主键更新）下完成首次导入 | 源与 Agent DB 按表逐块比较：行数一致、规范化后行哈希一致，零差异 |
| T2 增量 | C1（如支持）与 C5 各跑 ≥ 3 个周期，含无变化周期 | 每周期零差异；无变化周期零载荷、零目标改写；位点单调 |
| T3 故障注入 | 块读取中途杀 worker；目标提交后杀 worker；源容器重启；toxiproxy 断网/限速；SSH 跳板断开；日志清理超出保留；PG 18 slot 被作废 | 断点续跑不重复应用；超出保留时进入显式重基线而不是静默补洞；全部失败有回执 |
| T4 结构变化 | 加列、改类型、新建表、删表、改名 | 数据-only 周期零模型调用；结构变化只触发受影响社区的定向 Self-build |
| T5 Self-build 与 Pack | 全库 Self-build，含上下文预算器 | 覆盖率 100%；连通性指标写入审核摘要（[03](03-selfbuild-context-budget.md) §4.3）；Pack 往返一致 |
| T6 Gateway 对话 | 真实模型 Agent 按 `oo share-prompt` 提示词绑定，回答 20 道金标准题 | 每题答案与 gold 一致；失败/NULL/截断如实标注；不出现把 NULL 写成 0 |
| T7 只读证明 | 源端审计（MySQL general log、PG `log_statement`、Oracle 统一审计）| 读取账号顶层只发出 SELECT、元数据读取与白名单读取过程；任何非系统 schema 对象无变化；会话状态语句（`ALTER SESSION SET`、`SET`）、引擎在系统 schema 上的登录/字典记账与审计轨迹写入、Oracle LogMiner 自有对象（D11a）允许但必须逐类归因列入证据；无法归因的记录判失败（Plan D12）；读取账号仅有只读权限 |
| T8 资源 | 记录源端连接数峰值、读取占空比、目标写入字节、模型 token 用量 | 不超过配置预算 |

T1–T4 是引擎 owner 每次提交前必跑的；T5–T6 在每个 sprint 末对 P0 全版本跑一次；T7、T8 随 T1–T4 自动收集。

## 5. 比对口径

- **值规范化**：数值按精度转十进制字符串；时间统一到 UTC 微秒并保留原时区标注；Oracle 空串与 NULL 按源语义记录；浮点按位比较而非四舍五入。
- **分块比对**：按主键（或选定的唯一键）排序后分块计算哈希，差异块再逐行定位，报告首个差异行的键。
- **一致性等级**：比对报告声明本次结果的一致性等级（T/S1/S0/B），与 Gateway 暴露的等级一致。

## 6. 运行入口

```text
testing/engines/<engine>/<version>/compose.yaml   固定 digest 的单引擎夹具
testing/engines/<engine>/seed/                     方言 DDL 与数据生成
testing/engines/gold/                              共享金标准题与答案生成 SQL
oo-test matrix --engine mysql --versions 5.7,8.0,8.4,9 --suites T0-T4
oo-test matrix --all --suites T0-T8                 夜间全量（amd64 目标自动走仿真）
```

`oo-test` 是现有 `testing/infra` 与各 `tools/*-preview` 测试脚本的统一入口（Plan S0 任务）。每次运行生成 `evidence/<run-id>/`：原始退出码、比对报告、资源回执、SHA256SUMS。夜间回归失败自动在矩阵中降级并通知 owner（沿用 database-support/05 的降级规则）。

## 7. 外部参考（2026-11-03 检索）

- gvenzl Oracle XE 镜像与 Apple Silicon 说明：https://github.com/gvenzl/oci-oracle-xe
- SQL Server 2025 CU1 修复 Docker Desktop AVX 问题：https://www.nocentino.com/posts/2026-02-02-sql-server-2025-cu1-fixes-avx-issue
- Azure SQL Edge 退役：https://azure.microsoft.com/en-ca/updates/?id=azure-sql-edge-retirement
- Db2 社区镜像迁至 icr.io：https://community.ibm.com/community/user/blogs/vijaya-katikireddy/2023/02/04/db2-community
- macOS 27 与 Rosetta 2：https://www.macrumors.com/2026/10/26/macos-27-2-stops-removing-rosetta/
