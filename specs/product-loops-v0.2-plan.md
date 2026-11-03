# Open Ontology v0.2 产品闭环开发 Plan

> Status: Accepted（2026-11-03 人类决策见 §9）
> Owner: Open Ontology Engineering
> Written: 2026-11-03
> 基线：origin/master `6bbb368d4`；[v0.1 Spec](open-ontology-v0.1-spec.md)（来源基线 revision 5）；[支持矩阵](../docs/database-support/support-matrix.yaml)
> 设计输入：[docs/product/01–05](../docs/product/)

本 Plan 只覆盖**产品侧**。客户交付线另行管理；其通用经验已脱敏写入 docs/product/02、03。本 Plan 不复制 v0.1 Plan 的任务卡规范，任务卡字段沿用 `.codex/skills/codex-grok/references/task-template.md`。

## 0. 一页结论

| 闭环 | 一句话 | 收敛标志 |
|---|---|---|
| **L1 开源易用** | `oo init` 填库、填 key → 自动拉全量 → Ontology 可视化 + Pack → Gateway + token → 一段提示词交给用户的 Agent | 干净机器上不改任何文件，3 个 P0 引擎各跑通一次，另一个 Agent 按提示词绑定并答对金标准题 |
| **L2 Gateway ↔ CLI** | Gateway 跟随最新发布；token 可签发、续期、轮换、吊销、绑定；CLI 首次 `oo login` 绑定 | issue/renew/rotate/revoke/enroll 全部有成功与失败用例；CLI 到期预警；无需手工改表 |
| **L3 数据库矩阵** | MySQL/PG/Oracle 多版本全链 verified，另 13 个引擎并行推进 | P0 全部版本 T0–T8 证据包通过并由证据生成矩阵；P1 如实定级 |
| L4 拉取鲁棒性（支撑 L1/L3） | 分块、并行、水位、C5/C1、故障恢复统一进产品 | 05 的 T1–T4 在 P0 全版本零差异 |
| L5 Self-build 规模化（支撑 L1/L3） | 上下文预算器、社区切批、全局实体登记、连通性门禁 | 上千表夹具一次跑通，覆盖 100%，连通性指标进入审核 |

## 1. 起点（2026-11-03 实测与盘点）

| 领域 | 已有 | 缺口 |
|---|---|---|
| CLI | `oo` 查询命令；npm 包 `@reduxxx888/ontology-cli@0.1.2`（darwin-arm64、linux-x64）；`configure`、业务时区 | 无 init/TUI、login、whoami、续期、share-prompt |
| 编排 | `tools/source-onboarding`、`product_flow.py`、`minimal-product-loop`（仅 PG、仅 draft） | 无一条命令全链；大量私有 JSON 与 Python 胶水 |
| SSH | 无（`ssh` 被当作非法输入） | 跳板隧道、远程部署 |
| 模型 | 只认 loopback `/v1/chat/completions` + 白名单 | 用户自填 provider；出口代理 |
| Self-build | Python 路线可跑约 660 表；Rust Builder `MAX_TABLES=64` | 统一预算器、社区切批、连通性 |
| Gateway | 不透明 token、hash 存储、吊销四级、Pack 热加载 | renew/rotate/enroll、自助续期、到期监控、`/v1/meta` |
| 部署 | `deploy/product-stack` 需自建镜像，standard 依赖 runsc | 预构建多架构镜像，lite 档 |
| 连接器 | MySQL/Oracle 在 `crates/oo-connectors`；PG 与其余 7 引擎在 `tools/*-preview` | 无统一引擎接口与版本能力探测 |
| 测试 | `testing/infra/compose`（PG17、MySQL 8.4、toxiproxy 等）；各 preview 自带脚本 | 无多版本矩阵、无统一入口、amd64-only 引擎无固定运行方式 |
| 证据 | MySQL 5.7.44/8.4.10、PG 17.10、Oracle Free 23.x 有本地全链作者证据 | 矩阵内无任何 verified |

## 2. 工作方式

按 [docs/product/01](../docs/product/01-working-model.md)。要点：

- 主线程负责契约、集成、短修复、验收与文档；子智能体只领长程独占任务卡。
- 同时在跑的执行者：S0–S1 ≤ 6（见 ledger E016），之后 ≤ 5。
- 状态只看 `execution/v0.2/state.json`（覆盖写），人类指令记入 `execution/v0.2/decisions.md`，派发、交付、审阅、合入、sprint 退出、偏离与更正逐条追加到 `execution/v0.2/ledger.md`；`TODO.md` 由 `execution/v0.2/render_todo.py` 生成，每次状态变化后运行并以 `--check` 校验。
- 提交、分支与最终 PR 按 §10、§11。
- 普通功能不单独审阅；每个闭环收敛时对固定 SHA 做一次独立审阅。

### 2.1 车道与写域

| 车道 | owner | 独占写域 | 依赖 |
|---|---|---|---|
| M 主线程 | 协调者 | 根 `Cargo.toml`/`Cargo.lock`、`migrations/`、`crates/oo-connectors/src/engine/`（公共接口）、`support-matrix.yaml`、docs、`execution/v0.2/` | — |
| A 接入与编排 | 执行者 A | `crates/oo-cli/src/admin/`（init/up/doctor/build/status/ontology/share-prompt）、`crates/oo-stack/`（新）、`deploy/lite/`、`tools/model-egress/`（新） | M 的引擎接口 |
| B Gateway 与绑定 | 执行者 B | `tools/gateway-token-store/`、`crates/oo-gateway/src/auth*`、`crates/oo-cli/src/login*`、`packages/cli/` | — |
| C 引擎 MySQL 族 | 执行者 C | `crates/oo-connectors/src/mysql/`、`testing/engines/mysql/`、`testing/engines/mariadb/`、`testing/engines/tidb/`、`testing/engines/oceanbase/` | M 的引擎接口、H |
| D 引擎 PG 族 | 执行者 D | `crates/oo-connectors/src/postgres/`（由 `tools/postgres-preview` 收敛）、`testing/engines/postgres/`、`testing/engines/opengauss/` | 同上 |
| E 引擎 Oracle | 执行者 E | `crates/oo-connectors/src/oracle/`、`testing/engines/oracle/` | 同上 |
| G Self-build 预算 | 执行者 G | `crates/oo-builder/src/budget*`、`crates/oo-builder/src/community*`、`tools/pg-builder/` | M |
| H 测试基础 | 主线程或执行者 H | `testing/engines/gold/`、`testing/engines/seed-common/`、`tools/oo-test/`（新）、`ci/` | — |

并发编排：S0–S1 运行 A、B、C、D、E（5 条）；G 在 S1 中段 B 的核心接口交付后接替 B 的槽位；H 在 S0 由主线程完成。P1 引擎在 S3 起由 C/D/E 在各自 P0 引擎 verified 后依次接手（C→MariaDB/TiDB/OceanBase，D→openGauss，E→SQL Server/Db2），新增 OLAP/文档类引擎车道 F 在 S3 释放槽位后开启（ClickHouse/Doris/StarRocks/MongoDB/OpenSearch，由 `tools/*-preview` 收敛）。

## 3. Sprint 划分

每个 sprint 约一周；日期为建议，按交付推进，不按日历宣称完成。

**跨 sprint 规则**：车道可在自己的分支上继续做下一 sprint 的任务，但下一 sprint 的任务在本 sprint 退出被记为 ledger 的 `sprint-exit` 行之前，状态不得超过 `author_complete`，也不得合入集成分支（由 `render_todo.py --check` 强制）。

| Sprint | 建议时间 | 主题 | sprint 退出 |
|---|---|---|---|
| S0 | 11-04 – 11-10 | 地基：引擎接口、夹具、测试入口、状态文件、镜像矩阵 | `oo-test matrix` 能在 P0 全版本起库、灌夹具、跑 T0 |
| S1 | 11-11 – 11-17 | P0 全量快照 + 预检；token 生命周期 API；模型出口代理 | P0 全版本 T1 零差异；token 五种操作 API 与测试通过 |
| S2 | 11-18 – 11-24 | 增量（水位、C5、C1）；`oo init` TUI + `oo up` lite；预算器 v1 | P0 全版本 T2 通过；本地 lite 一条命令跑到 draft |
| S3 | 11-25 – 12-01 | Self-build 规模化与连通性；`oo login`/`whoami`/`share-prompt`；SSH 跳板；P1 启动 | 上千表夹具 T5 通过；L2 闭环收敛（独立审阅） |
| S4 | 12-02 – 12-08 | L1 端到端：自审、发布、Gateway、提示词；远程 SSH 部署；P1 快照 | 3 个 P0 引擎在干净机器上 L1 跑通 |
| S5 | 12-09 – 12-15 | 加固：T3/T4 故障与结构变化、T7/T8、P1 增量；夜间矩阵 | P0 全版本 T0–T8 证据包；矩阵由证据生成 |
| S6 | 12-16 – 12-22 | 发布：打包、多架构镜像、Quickstart、独立审阅、npm 发布 | L1、L3 闭环收敛；v0.2 Release ready |

## 4. 完整 TODO

ID 格式 `S<sprint>-<车道>-<序号>`。“测试 / 退出”列末尾的 `AT-*` 是 Spec 第 7B 章的验收用例，由该项执行。“测试”列指 [05](../docs/product/05-version-matrix-and-testing.md) 的套件或本项专属用例；每项退出时附固定 SHA 与证据。

### S0 地基

| ID | 车道 | 任务 | 交付 | 测试 / 退出 |
|---|---|---|---|---|
| S0-M-01 | M | 建 `execution/v0.2/state.json` 与 `decisions.md`；从本 Plan 生成任务卡 | 状态文件与渲染脚本 | TODO 由状态文件生成，`--check` 通过 |
| S0-M-02 | M | 定义 `SourceEngine` 公共接口：`probe/capabilities(version)/catalog/plan_chunks/read_chunk/watermark/change_stream/digest_range` | `crates/oo-connectors/src/engine/` + 文档 | 接口编译；MySQL 现有实现适配通过现有测试 |
| S0-M-03 | M | 版本分派表格式与 `doctor` 输出契约（含 DBA GRANT 清单） | `engine/capability.rs`、JSON schema | schema 测试 |
| S0-M-04 | M | `stack.yaml`（`oo.stack/v1`）schema 与 secret_ref 规则 | schema + 示例 | 校验器单测：拒绝内联密码 |
| S0-H-01 | H | 中性合成夹具生成器（[05 §3](../docs/product/05-version-matrix-and-testing.md) 全部形状）+ 方言 DDL：MySQL、PG、Oracle | `testing/engines/seed-common/`、各引擎 `seed/` | 三引擎灌数成功，行数与 gold 一致 |
| S0-H-02 | H | 20 道金标准题与 gold 生成 SQL | `testing/engines/gold/` | gold 在三引擎上结果一致 |
| S0-H-03 | H | `oo-test` 统一入口：起库（固定 digest）、灌数、跑套件、写证据包 | `tools/oo-test/` | 本地跑通 MySQL 8.4、PG 17、Oracle 23ai 的 T0 |
| S0-H-04 | H | P0 镜像 digest 锁定：MySQL 5.7/8.0/8.4/9、PG 12–18、Oracle 23ai/11g XE/21c XE（19c 待授权） | `images.pins`、`images.lock.json` | `docker buildx imagetools inspect` 复核 |
| S0-H-05 | H | amd64 仿真 runner：`oo-test --platform linux/amd64` 在本机 Docker Desktop（Rosetta）运行 amd64-only 镜像；保留 `--runner remote` 接口供日后接入云主机 | `tools/oo-test` | MySQL 5.7 与 Oracle 11g XE 在本机仿真下跑通 T0 |
| S0-C-01 | C | MySQL 适配 `SourceEngine`；`capabilities` 覆盖 5.7/8.0/8.4/9.x 差异（认证、`SHOW BINARY LOG STATUS`、tagged GTID、`expire_logs_days`） | `mysql/` | T0 四版本；版本分派单测 |
| S0-D-01 | D | PG 连接器从 `tools/postgres-preview` 收敛到 `crates/oo-connectors/src/postgres/`，适配接口 | `postgres/` | 现有 PG 测试迁移后通过；T0 PG 13–18 |
| S0-E-01 | E | Oracle 适配接口；`capabilities` 覆盖 11g/12c+/19c/21c/23ai（CDB/PDB、common user、CONTINUOUS_MINE、字典策略） | `oracle/` | T0 23ai 原生、11g/21c amd64 仿真 |
| S0-A-01 | A | 管理端命令骨架：`oo init/up/down/status/doctor/build` 参数与 `--help`，非 TTY 全非交互 | `oo-cli/src/admin/` | CLI 契约测试：错误参数清晰且不泄密 |
| S0-B-01 | B | token-store 迁移设计：renew/rotate/enroll 字段与审计事件 | 迁移 SQL（交 M 合入）、契约文档 | 迁移在空库与已有库上可重复应用，旧 checksum 不变 |

### S1 P0 全量快照、token API、模型出口

| ID | 车道 | 任务 | 交付 | 测试 / 退出 |
|---|---|---|---|---|
| S1-C-01 | C | MySQL 分块：键形态识别、分布因子与抽样切分、字符串主键源端比较、复合主键改写、无键表策略、自适应块 | `mysql/chunk.rs` 扩展 | T1 四版本零差异（并发写入下）；AT-ENG-01 |
| S1-C-02 | C | 源库保护：占空比、`Threads_running` 暂停、语句超时双保险、`net_*_timeout`、连接余量限流 | 同上 | T8：连接峰值与占空比在预算内 |
| S1-D-01 | D | PG 分块：keyset + 单快照内 ctid（14+）；`statement_timeout`；活跃会话保护 | `postgres/` | T1 PG 13–18 零差异；AT-ENG-01 |
| S1-E-01 | E | Oracle 分块：keyset + extents 生成 ROWID 范围；`ORA-01555` 缩块重试；空串语义 | `oracle/chunk.rs` | T1 23ai/11g/21c 零差异；AT-ENG-01 |
| S1-M-01 | M | 统一暂存→原子替换→发布；不可变父存储 + 变化块组合；目标写入计量 | `crates/oo-pipeline` | 两次真实 SIGKILL 恢复无重复应用 |
| S1-M-02 | M | 有界滚动调度（并发 1–4）、失败即停派、周期预算文件 | `tools/job-worker` | 调度单测；慢任务不阻塞其他槽 |
| S1-B-01 | B | Gateway 管理 API：issue/list/renew/rotate/revoke/enroll；自助 `POST /v1/auth/renew`（策略可关） | `oo-gateway`、`gateway-token-store` | 成功与失败用例：过期、已吊销、重复兑换、超最大寿命、错误 audience；AT-TOK-01 |
| S1-B-02 | B | `GET /v1/meta`（协议版本、最低 CLI 版本、发布与新鲜度）；mTLS 证书与 token 到期纳入 `/v1/health` | 同上 | 到期 ≤14 天告警用例；AT-TOK-03 |
| S1-A-01 | A | `tools/model-egress`：loopback 监听、持 key、OpenAI 兼容/Anthropic 方言、限流、统一额度错误码、计量；能力探测（W、O_max、JSON 模式） | 新工具 | 对两种方言的录制回放测试；key 不出现在日志；AT-ONB-04 |
| S1-A-02 | A | `oo doctor` 接入三引擎 `probe/capabilities`，输出分块计划、行字节估算、DBA 清单 | admin | T0 P0 全版本 |

### S2 增量、TUI、lite 一键、预算器 v1

| ID | 车道 | 任务 | 交付 | 测试 / 退出 |
|---|---|---|---|---|
| S2-C-01 | C | MySQL 只读水位（GTID / file-pos）与块级日志修正；首次全量前记 L0 并持续消费 | `mysql/` | T1 + 日志修正用例；T2 C1 四版本；AT-ENG-01 |
| S2-C-02 | C | MySQL C5 统一到接口；C5→C1 多来源多绑定交接屏障；保留期按实际最早日志预检 | `mysql/`、`oo-pipeline` | T2 C5；交接用例；保留期不足时显式重基线 |
| S2-D-01 | D | PG 逻辑复制：slot/publication 预检、proto 版本协商、`publish_via_partition_root`、18 slot 作废检测与重基线 | `postgres/` | T2 PG 13–18；AT-ENG-01 |
| S2-E-01 | E | Oracle LogMiner 位点：19c+ 自管日志列表、CDB/PDB、SCN 水位；C5 统一到接口 | `oracle/` | T2 23ai C1+C5；11g/21c C5；AT-ENG-01 |
| S2-A-01 | A | `oo init` TUI（ratatui）：7 步向导，结果写 `stack.yaml`；秘密写 0600 或 keychain | admin | 录屏式交互测试；TTY 与 `--config` 结果一致；秘密扫描；AT-ONB-02 |
| S2-A-02 | A | `deploy/lite` compose 与 `oo up/down/status`：Agent DB、控制库、worker、Builder、Gateway、model-egress、scheduler；镜像 digest 锁定 | lite 档 | 本地一条命令从三引擎夹具跑到 draft |
| S2-G-01 | G | 预算器 v1：能力探测接入、双向预算、usage 校准、降级顺序、NDJSON 输出与缺失项重请求 | `oo-builder/src/budget*` | 单测；对 300 列宽表不截断 |
| S2-G-02 | G | 移除项目级 `MAX_TABLES=64`，改为批级预算；Python 与 Rust 路线统一到同一预算器 | 同上 | 660 表规模夹具 T5 覆盖 100%；AT-BLD-01 |

### S3 规模化 Self-build、CLI 绑定、SSH、P1 启动

| ID | 车道 | 任务 | 交付 | 测试 / 退出 |
|---|---|---|---|---|
| S3-G-01 | G | 关系图（声明 FK + 包含度候选 + 命名）、分表族折叠、社区划分、枢纽签名 | `community*` | 上千表夹具：社区大小均在预算内 |
| S3-G-02 | G | 三遍生成与全局实体登记表；连通性门禁指标写入审核摘要 | 同上 | T5：外键全映射或有理由；孤立实体比例与连通分量报告；AT-BLD-01 |
| S3-G-03 | G | 语义依赖哈希按社区；数据-only 周期零模型调用 | `oo-builder/src/incremental.rs` | T4；AT-BLD-02 |
| S3-B-01 | B | `oo login`（绑定码 / 设备码）、`oo whoami`、到期预警、`/v1/meta` 版本协商与升级提示 | `oo-cli`、`packages/cli` | 端到端：enroll→login→query；过期后提示续期；AT-TOK-02 |
| S3-B-02 | B | `oo share-prompt [--role] [--agent claude|cursor|codex] [--raw-token]` | 同上 | 另一个 Agent 按提示词完成绑定（T6 前置） |
| S3-A-01 | A | SSH 跳板：`russh` direct-tcpip、主机指纹确认与固定、断线块级续读 | `oo-stack`、worker | T3 SSH 断开用例；指纹不匹配拒绝；AT-ONB-03 |
| S3-C-02 | C | P1：MariaDB 10.6/10.11/11.4/11.8 接口适配与 T0–T2（由 `tools/mariadb-preview` 收敛） | `mysql/`（MariaDB 方言） | T0–T2 |
| S3-E-02 | E | P1：SQL Server 2019/2022/2025 快照 + CT/CDC（amd64 仿真） | `mssql/`（由 preview 收敛） | T0–T1 |
| S3-M-01 | M | **L2 闭环独立审阅**（固定 SHA） | 审阅报告 | 审阅发现的阻断项清零 |

### S4 L1 端到端、远程部署、P1 扩展

| ID | 车道 | 任务 | 交付 | 测试 / 退出 |
|---|---|---|---|---|
| S4-A-01 | A | 所有者自审 TUI：实体、关系、指标、敏感列分类的逐类确认；`review_decision` 记录 `mode=self` | admin | 不可跳过审核发布；自动接受不冒充人工 |
| S4-A-02 | A | `oo build` 全链：拉取→Self-build→自审→发布→Gateway→签发首个 token→生成提示词；`--resume` | admin | 中途中断各阶段均可续跑 |
| S4-A-03 | A | `oo ontology open/export --format html|mermaid|yaml`：离线自包含 HTML（基于 `relationships/graph.json`） | admin、`oo-builder` | 无网络打开；上千实体可交互 |
| S4-A-04 | A | 远程部署：`oo up --target ssh` 安装 compose 栈与 systemd timer | `oo-stack` | 本机起一个带 sshd 与 Docker 的干净 Linux 容器（amd64 与 arm64 各一）作为目标主机，部署成功；云主机就绪后补一次真机；AT-ONB-03 |
| S4-M-01 | M | 发布跟随：每周期发布新数据代际，Gateway 原子切换指针，`/v1/freshness` 报告 | `release-coordinator`、`oo-gateway` | 连续 3 周期对话中途无错误；AT-TOK-03 |
| S4-M-02 | M | **L1 干净机器验收**：macOS arm64 与 Linux amd64 各一，MySQL/PG/Oracle 各一次 | 证据包 | [04 §7](../docs/product/04-onboarding-and-gateway-binding.md) 第 1–4 条；AT-ONB-01 |
| S4-C-03 | C | P1：TiDB、OceanBase 快照 + C5 | 方言模块 | T0–T2 |
| S4-D-02 | D | P1：openGauss 快照 + C5（认证与逻辑解码分叉单独处理） | `postgres/`（方言） | T0–T2 |
| S4-F-01 | F | P1：ClickHouse/Doris/StarRocks 快照 + 分区刷新（由 preview 收敛到接口） | `olap/` | T0–T2 |

### S5 加固与证据

| ID | 车道 | 任务 | 交付 | 测试 / 退出 |
|---|---|---|---|---|
| S5-C-04 | C | MySQL 5.7/8.0/8.4/9.x 的 T3 故障注入、T4 结构变化 | 证据包 | 全部通过或如实降级；AT-ENG-02 |
| S5-D-03 | D | PG 13–18 的 T3、T4 | 证据包 | 全部通过或如实降级；AT-ENG-02 |
| S5-E-04 | E | Oracle 23ai/11g/21c（及 19c 如 D2 就绪）的 T3、T4 | 证据包 | 全部通过或如实降级；AT-ENG-02 |
| S5-H-01 | H | T7 只读证明自动化（general log / log_statement / 统一审计） | `oo-test` | P0 全版本；AT-ENG-03 |
| S5-H-02 | H | 夜间矩阵（本机原生 + amd64 仿真），失败自动降级并通知 owner | `ci/` | 连续 3 晚稳定 |
| S5-M-01 | M | 证据包 → `support-matrix.yaml` 生成器；禁止手写 verified | 生成脚本 | 生成结果与证据一致；AT-ENG-03 |
| S5-G-01 | G | T5/T6 在 P0 全版本：20 道金标准题由真实模型 Agent 回答 | 证据包 | 全部答对；失败/NULL/截断如实标注；AT-ENG-03 |
| S5-F-02 | F | P1：MongoDB change stream、OpenSearch/ES 快照 | 模块 | T0–T2 |
| S5-E-03 | E | P2：Db2 快照；达梦/Kingbase 视镜像与许可 | 模块 | T0–T1 |

### S6 发布

| ID | 车道 | 任务 | 交付 | 测试 / 退出 |
|---|---|---|---|---|
| S6-M-01 | M | 多架构镜像 CI 构建与签名；lite 档离线安装包 | CI | 两架构 fresh 安装 |
| S6-M-02 | M | Quickstart（三分钟上手）、`oo --help` 全命令、故障排查、DBA 权限清单（按引擎版本） | docs | 新人按文档完成 L1 |
| S6-M-03 | M | **L1、L3 闭环独立审阅**（固定 SHA） | 审阅报告 | 阻断项清零 |
| S6-B-01 | B | npm 发布 `@reduxxx888/ontology-cli@0.2.0`：`npm pack` → 空目录安装 → 全命令 → 同一 tarball 发布；报告精确版本与升级命令 | 发布记录 | 两架构安装验证 |
| S6-M-04 | M | 核对本期所有设计变化均已当期回填原文档、Spec 与矩阵（D7）；来源基线升至最终 revision | docs、specs | 三个检查脚本通过；无“待回填”条目 |

## 5. 测试与校验方案

测试套件、夹具、镜像与比对口径见 [05](../docs/product/05-version-matrix-and-testing.md)。按闭环汇总：

| 闭环 | 必过测试 | 校验者 | 证据 |
|---|---|---|---|
| L1 | 干净机器 × 2 平台 × 3 P0 引擎：`oo init` → 提示词 → 另一 Agent 绑定 → T6 金标准题；`stack.yaml` 非交互复现；SSH 跳板与远程部署（目标为本机 sshd 容器，见 S4-A-04）各一次；秘密扫描 | 主线程 + 独立审阅 | 录屏/终端记录、证据包 |
| L2 | token 五种操作成功与失败用例；过期、吊销、重复兑换、超寿命、错误 audience；CLI 到期预警；`/v1/meta` 版本拒绝；mTLS 到期告警；发布切换期间对话不断 | 执行者 B + 独立审阅 | 测试报告 |
| L3 | P0 每个版本 T0–T8；P1 每个版本至少 T0–T2；矩阵由证据生成 | 夜间矩阵 + 主线程 | `evidence/<run-id>/` |
| L4 | T1–T4 在并发写入与故障注入下零差异 | 引擎 owner | 比对报告 |
| L5 | 上千表夹具 T5；连通性指标；数据-only 周期零模型调用 | 执行者 G | 审核摘要、调用计量 |

**每个 sprint 末**：主线程在 P0 全版本跑一次 T0–T5（本机原生 + amd64 仿真），把结果写入 `state.json`；未通过的项不进下个 sprint 的“已完成”。

**不算通过**：CLI 退出 0、HTTP 200、零测试、容器启动、只有作者自测、模型给出最终文字但未对 gold。

## 6. 退出条件（v0.2 Release ready）

1. L1、L2、L3 三个闭环均有固定 SHA 上的独立审阅，阻断项清零。
2. P0：MySQL 5.7/8.0/8.4/9.x、PG 13–18、Oracle 19c/23ai 全链 `verified`；Oracle 11g/21c 快照 + C5 `verified`；全部由证据包生成。
3. P1：各引擎按实际证据定级，不低于 `experimental` 的至少 8 个。
4. npm 包与多架构镜像发布，Quickstart 由未参与开发的人按文档完成 L1。
5. 源端只读证明、秘密扫描、资源预算在全部运行中无违例。

## 7. 风险

| 风险 | 影响 | 应对 |
|---|---|---|
| amd64-only 镜像在 Apple Silicon 仿真不稳，且 Rosetta 收缩 | MySQL 5.7、Oracle 11g/21c、SQL Server、Db2 测试慢或不可复现 | 本期按 D1 用本机仿真；`oo-test` 保留远程 runner 接口，必要时再购云主机 |
| Oracle 19c EE 镜像需登录与许可 | 19c verified 推迟 | 人类完成许可授权；否则 19c 维持 experimental 并如实标注 |
| 模型额度与限流 | Self-build、T6 中断 | 预算器 fail-fast + 断点续跑；测试用独立 key 与额度 |
| 大 schema 关系质量 | Ontology 孤岛 | 社区切批 + 实体登记 + 连通性门禁进入审核 |
| lite 档无 runsc | 单机安全边界弱于 standard | 文档明确“单所有者”定位；standard 档保留 |
| 车道之间接口变化 | 返工 | S0 冻结 `SourceEngine` 与 `stack.yaml`；变化先改卡 |
| 客户交付线与产品线互相干扰 | 误改客户服务或泄露客户信息 | 产品开发不访问客户环境；同步遵守 `docs/product/source-sync.md` |

## 8. 不在本期

Web UI；短期 access + refresh 轮换 token（v0.3 评估）；云数仓（Snowflake/BigQuery/Databricks）；Redis 等 KV；多租户 SaaS；客户环境迁移与 ECS 部署（交付线负责）。

## 9. 已定决策（2026-11-03）

| # | 事项 | 决定 | 影响 |
|---|---|---|---|
| D1 | x86 测试机 | 暂不购买云主机（人类决定）。amd64-only 镜像（MySQL 5.7、Oracle XE 11g/21c、SQL Server、Db2）在本机 Docker Desktop 以 `--platform linux/amd64`（Rosetta）运行；已实测可用 | S0-H-05 改为本地 amd64 仿真 runner；夜间矩阵在本机执行；Rosetta 收缩风险保留在 §7 |
| D2 | Oracle 19c EE | 需要 Oracle 账户签入后在 container-registry.oracle.com 接受许可。账户注册须由人类本人提交（代理不得代建账户、不得代输密码）；签入后由主线程经浏览器接受许可并生成 registry Auth Token（0600 私有文件） | 在账户就绪前 19c 维持 `experimental`；23ai Free 与 11g/21c XE 不受影响 |
| D3 | npm 包 | 沿用 `@reduxxx888/ontology-cli`，本期发布 `0.2.0`（语义化版本；补丁按 `0.2.x`） | S6-B-01 |
| D4 | 测试模型 | `gpt-6.1-sol`，OpenAI 兼容端点由人类提供；key 只存 `~/.config/open-ontology/secrets/` 下 0600 文件，经 `secret_ref` 引用，绝不进仓库、argv、日志 | 实测：代理对每次调用注入约 2.5k token 系统提示；同名模型存在 272k 与 1.05M 两种上下文路由，最大输出 128k；预算器默认按 272k 并以首调探测与 usage 校准（03 §3） |
| D5 | 达梦 / KingbaseES | 人类授权获取；如官方下载无需注册则直接获取，需注册则同 D2 由人类本人完成注册 | P2，不阻塞退出 |
| D6 | 所有者自审 | 作为开源单所有者场景下发布门禁的合规形态，记录 `review_decision.mode=self`；多人部署仍要求独立审阅者 | S4-A-01 |
| D7 | 文档与基线 | 设计变化当期回填原文档，不延后；来源基线随文档同步升级（revision 6 起纳入 `docs/product/*`），检查脚本按新范围更新 | 本 Plan 每个 sprint 末 `check_spec`、`check_plan`、`check_v02_plan` 与 `render_todo.py --check` 必须通过 |
| D8 | Oracle C1 写入分类 | LogMiner 只用 `online_catalog` 字典策略，视为 R1（源端零写，只读 common user）；需在源端构建字典的 `redo_log_catalog` 属 R1w，默认禁用；结构变化期间 online_catalog 无法解析的事件触发该表重基线 | Oracle 19c/23ai C1 可按 R1 进入 verified；S2-E-01 必须用源端审计（T7）证明零写 |
| D9 | 服务账号 token 寿命 | 默认有效期与默认最大寿命均为 90 天，项目策略可配置最大寿命至 365 天；自助续期不超过“首次签发 + 最大寿命” | REQ-TOK-01/02；docs/07 §3.4 |
| D10 | MySQL 进程内 binlog C1 的依赖 | `mysql_async` 的 binlog 支持引入 bitvec 与 zstd 栈；zstd 0.13.3 / zstd-safe 7.3.0 有 RUSTSEC-2026-0391/0398/0403/0410，修复版 2026-11-01 发布，未满 7 天，且在 CODEOWNERS 仍为 `OWNER_PENDING` 时安全例外不可用（supply-chain/policy.md §4）。因此 C1 放在非默认 feature `mysql-binlog` 后开发与测试，锁文件钉修复版（cargo audit 0 漏洞）；2026-11-08T14:12:08Z 之后且许可证文件核验完成，才在默认制品启用 | S2-C-01/S2-C-02 可在 feature 下验收；默认制品启用列入 S6-M-04；记录 `supply-chain/mysql-binlog-audit.json` |
| D11 | Oracle LogMiner 的源端写入分类（修订 D8） | 23ai 实测：LogMiner 会话由 Oracle 内部包递归（depth > 0）对 SYS 拥有的会话级全局临时表 `SYS.LOGMNR_GT_*` 执行 DELETE/COMMIT，另有驱动的会话时区设置；读取账号本身不发出任何 DML/DDL，不写业务表或持久对象。归类为“R1，含 Oracle 内部递归临时写”：T7 只放行这一显式类别（递归深度 > 0、对象限 `SYS.LOGMNR_GT_*`、会话设置语句），其余任何写入仍判失败；`oo doctor` 与 DBA 清单向用户披露该行为；Oracle C1 不进入 `sync: auto` 默认选择，需用户显式选用 | S2-E-01、S5-E-04、AT-ENG-03；docs/database-support/04 §5.3 与 docs/product/02 §7 同步 |

## 10. 提交规范

### 10.1 分支

- 集成分支 `v0.2/product-loops`（自 origin/master `6bbb368d4` 起）。只有主线程向它写入。
- 每条车道一个 worktree 与分支 `v0.2/<车道>-<主题>`，例如 `v0.2/c-mysql-chunking`，从集成分支最新提交切出。
- 车道分支完成一张任务卡后，由主线程 rebase 到集成分支并 fast-forward 合入，保持线性历史；不在集成分支上做 merge commit，不 force-push 集成分支。
- 唯一例外：尚未推送的集成分支提交中发现秘密或客户信息时，主线程可在推送前改写该提交将其移除，并在 ledger 记一条 `deviation`（见 E005）；已推送的历史永不改写，只能另行处置。
- 不从旧主工作区（`chore/product-sync-*` 系列）挑拣或推送任何提交；它带有历史大对象。

### 10.2 提交范围

| 允许提交 | 禁止提交 |
|---|---|
| 产品源码、迁移（新文件，不改旧迁移字节）、测试、中性合成夹具生成器与 gold 生成 SQL | 秘密（模型 key、token、密码、私钥、registry token）与任何 `secret_ref` 指向的文件 |
| 文档、Spec、Plan、检查脚本、CI 配置、镜像 digest 锁 | 客户名称、端点、schema、原始行、业务报表、客户 Skill |
| 证据摘要（`execution/v0.2/evidence-index.json` 中的 run-id、sha256、结论） | 证据原件、数据库转储、容器卷、日志、`target/`、二进制制品、> 1 MiB 的生成文件 |
| `execution/v0.2/state.json`、`decisions.md` | 运行状态、调度状态、本地绝对路径 |

证据原件留在 git 忽略的 `evidence/<run-id>/`，提交里只记录其 sha256 与结论。

### 10.3 提交标准

1. **一个提交一个逻辑变化**，能单独审阅、单独回退。重构与行为变化分开提交；格式化不与功能混合。
2. **提交信息**用 Conventional Commits：`<type>(<scope>): <摘要>`，type 取 `feat|fix|refactor|style|test|docs|build|ci|chore|perf`（`style` 仅用于不改行为的格式化），scope 取 crate 或目录（如 `connectors/mysql`、`gateway`、`cli`、`builder`、`docs`、`specs`）。摘要用英文祈使句，≤ 72 字符。
3. **正文**写清：为什么改、改了什么行为、怎么验证（命令与结果摘要）；有失败模式或已知限制时写明。
4. **尾注**：`Plan: <TODO ID>`（可多个）；证据有对应运行时加 `Evidence: <run-id>`；按仓库约定加 `Co-Authored-By`。
5. **每个提交都可构建**：`cargo check --workspace --locked` 通过；改动涉及的 crate 的单元测试通过；改文档时三个检查脚本通过。不允许“下一个提交再修”的中间坏状态进入集成分支。
6. **提交前扫描**：`git diff --cached` 过一遍秘密与客户信息扫描（沿用 `docs/product/source-sync.md` 的扫描规则）和大文件检查。
7. 不使用 `--no-verify`，不改写已合入集成分支的历史。

### 10.4 车道交付与合入

执行者交付：车道分支上的一组符合 10.3 的提交 + 证据包 run-id。执行者为本地 Codex 时由 `execution/v0.2/dispatch.sh` 派发任务卡（`execution/v0.2/cards/`），运行记录在 git 之外。合入只经 `execution/v0.2/integrate.sh`：rebase、提交信息与 Plan 尾注、秘密与客户信息、> 1 MiB 文件、`cargo check --workspace --locked`、被改 crate 的测试与三个文档检查全部通过后 fast-forward。主线程合入前另行复核：提交粒度与信息、构建与相关测试、证据 sha256、写域未越界。复核不通过退回车道，不在集成分支上补丁式修复别人的提交。

## 11. 最终 PR

所有闭环（L1–L5）达到 §6 退出条件后，主线程从 `v0.2/product-loops` 向 `master` 开一个 PR 供人类审核。PR 描述包含：

- 闭环与退出条件逐项对照，每项链接证据 run-id 与审阅报告；
- 支持矩阵前后差异（由证据生成）；
- 提交清单按车道分组（`git log --oneline master..v0.2/product-loops`）；
- 未达成项与降级项的如实列表；
- 升级说明：npm `0.2.0` 安装命令、迁移、配置变化。

PR 在独立审阅完成前不合并；合并方式由人类决定。
