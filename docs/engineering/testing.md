# 测试

> Last reviewed: 2026-11-16

## 1. 套件（T0–T10）

| 套件 | 验证什么 | 入口 |
|---|---|---|
| T0 | 产品 doctor：权限、TLS、能力探测 | `tools/oo-test/oo-test matrix --suites T0 --hook T0=tools/oo-test/t0.py` |
| T1 / T8 | 快照逐行相等；源库负载（连接数、占空比） | `matrix --suites T1,T8 --hook T1=tools/oo-test/t1.py` |
| T2 | C1 / C5 增量、交接、位点不回退 | `tools/oo-test/t2.py`（PG）、`t2_p0.py`（MySQL / Oracle） |
| T3 / T4 | 故障（重启、断网、SIGKILL、日志过期）；结构变化 | `tools/oo-test/oo-test t3t4` |
| T5 / T6 | 发布周期；金标对话（20 题） | `tools/oo-test/oo-test t6 --execute` |
| T7 | 审计源库收到的每条语句，确认只读 | `tools/oo-test/t7.py` |
| T9 | 按表隔离、状态语义 | `tools/oo-test/t9_*.py` |
| T10 | 生产影响（D24：前台 p95 ≤ +20%，p99 ≤ +50%，吞吐 ≥ −5%，零错误） | `tools/oo-test/t10.py`，方法见 `execution/v0.2/cards/T10-production-impact.md` |
| QS | 快速体验 QS-01…05：从安装到首答，含拒绝、审计、MCP、清理 | `tools/oo-test/test_quickstart_live.py` |
| A/B | 只给 schema（C0）与加上审核本体（C3）的对照；同一模型，确定性比对 | `oo-eval ab --arms c0,c3`，重放门禁见 [ci.md](ci.md) |

P0 版本矩阵：PostgreSQL 13–18、MySQL 5.7–9.5、Oracle 11g / 19c / 21c / 23ai，见 `docs/database-support/`。

## 2. 分层快速测试（日常用）

全量矩阵串行跑要十几个小时，日常不跑全量。分层设计见 [测试加速方案](../../specs/test-acceleration-plan.md)，实现与验收见 `tools/oo-test/TA-01-ACCEL.md`。

| 层 | 证明什么 | 命令 | 频率 |
|---|---|---|---|
| L0 | 与引擎无关的逻辑（编译、裁决、Gateway 准入、哈希、幂等），不依赖容器 | `oo-test fast --layer L0 --run-id ID` | 每次提交 |
| L1 | 每个引擎每个版本的方言能力：目录、类型、快照、CDC 起点、审计 | `oo-test fast --layer L1 [--base REF] --run-id ID` | 每个 PR（按改动影响选格子） |
| L2 | 每个引擎选一个代表版本，跑端到端 | `oo-test fast --layer L2 --run-id ID` | 每个 PR |
| L3 | 200 万行规模、长时间 T7、T8 长窗口、T10、真实故障 | 现有全量命令；`oo-test l3 plan/run/pairs/diff` | 夜间轮换，发版前跑一次全量 |

- **快照源库**：每个引擎版本只灌一次数据，后续用写时复制克隆。宿主机不支持 reflink 时自动退回普通复制，用的是哪种方式记录在 timing 回执里。
- **版本等价类**：每类选一个代表版本跑完整 L1，代表按天轮换；其余版本只跑能力探针。**安全相关断言**（审计、权限、注入、只读）在每个版本上都跑，不参与抽样。
- **故障点**：放在只供测试用的 cargo feature `failpoints` 里，发布构建里没有。每个故障点都配一条 L3 真实故障用例做对照。
- **阶段计时**：`oo-test timing` 汇总每个阶段的耗时。

## 3. 缺陷回放闸门

`oo-test gate --defects --run-id ID` 会把历史上真实修过的缺陷（`defects.json`）逐条反向注入到一个临时 worktree 里，然后跑本应发现它的快速子集，**要求每一条都失败**，也就是都被抓到。

- 修复一个新缺陷时，同时把它的变异加进语料。
- 改动测试分层或删减用例之前，先跑一遍这个闸门。
- 截至 2026-11-13，14/14 被抓到。

## 4. 什么时候跑全量

- 发版前：固定 SHA，跑全量 L3，产出对外"已验证"的证据。支持矩阵的等级只认 L3 和 L1 代表版本的证据。
- 改了连接器公共接口、迁移或发布链路时，对受影响的引擎跑全量。
- 快速层和全量层结论不一致时（`oo-test l3 diff`），必须查清原因：可能是快速层漏测，也可能是全量层本身有缺口。两种情况都发生过。

## 5. 主机资源约束

- 磁盘：开跑前低于 20 GB 不开始，运行中低于 15 GB 立即停。清理只删本项目 worktree 下的 `target/` 和自己打标签的容器、卷。
- 内存：调度器按容器内存预算装箱，上限是 Docker VM 内存的 70%。Oracle 容器同一时间最多 1 个，除非预算允许。曾经因为 4 路并行导致 OOM。
- T10 必须在空闲主机上测。测之前用 `docker stats` 确认其他容器空闲；源库和 Agent DB 不能共用磁盘。
- Oracle 11g 需要 thick worker 镜像（`testing/engines/oracle/worker/`）。Oracle archivelog 过期用例需要带 RMAN 的完整镜像。

## 6. 证据

证据写在 `evidence/<run-id>/` 下，这个目录不进 git。回执里记录二进制哈希、源码 SHA 和镜像 digest。要保留的证据先做秘密和私有词扫描。报告里只引用 run id 和哈希。
