# 产品侧文档（v0.2）

> Last reviewed: 2026-11-03

这一组文档把前一阶段在真实生产链路上积累的经验、外部检索结果和本期三个闭环的设计放在一起，供 [v0.2 Plan](../../specs/product-loops-v0.2-plan.md) 使用。

| # | 文档 | 内容 |
|---|---|---|
| 01 | [工作形态](01-working-model.md) | 主线程与子智能体分工、派单规则、单一状态文件、验收层级、开发基线、硬规则 |
| 02 | [数据拉取手册](02-ingestion-playbook.md) | 键形态与分块、并行与源库保护、只读水位、C5/C1 与交接、故障恢复、引擎版本差异 |
| 03 | [Self-build 上下文预算](03-selfbuild-context-budget.md) | 模型能力探测、双向预算、降级与截断修复、社区切批、全局实体登记、连通性门禁、增量 |
| 04 | [开源接入与 Gateway 绑定](04-onboarding-and-gateway-binding.md) | `oo init` TUI、`stack.yaml`、lite 部署、SSH、模型出口代理、token 生命周期、`oo login`、Agent 提示词 |
| 05 | [版本矩阵与测试](05-version-matrix-and-testing.md) | 16 个引擎的版本目标、镜像与运行位置、夹具、T0–T8 套件、比对口径 |
| — | [产品源同步规则](source-sync.md) | 交付仓库向产品仓库同步时允许与禁止的内容 |

## 对既有文档的修订

本期设计改变或补充了下列既有文档，已于 2026-11-03 当期回填原文，来源基线升至 revision 6（[记录](../../specs/source-baseline-revision-6.md)）。之后的设计变化同样当期回填（Plan D7）。

| 既有文档 | 现状 | 本期变化 |
|---|---|---|
| [03 数据源](../03-sources-and-ingestion.md)、[04 管线](../04-pipeline.md) | 以 Debezium 流式 + 分块快照为主设计；分块按整数键描述 | 补充字符串/复合/无键表分块、分布因子与抽样、只读水位、源库保护、增长块拆分（02） |
| [database-support/03](../database-support/03-change-capture-and-consistency.md) | 捕获目录与一致性等级 | 补充 C5 四层与 C5→C1 多来源交接屏障、保留期按实际最早日志预检（02 §5） |
| [database-support/04](../database-support/04-database-profiles.md)、[support-matrix.yaml](../database-support/support-matrix.yaml) | MySQL 5.7/8.x、PG 13–17、Oracle 19c/23ai | 加入 MySQL 9.x、PG 12/18、Oracle 11g/21c，以及 8.4/PG18/23ai 的版本差异（02 §7、05 §1）；verified 仅由证据生成 |
| [06 Ontology 层](../06-ontology-layer.md) | S0–S10 流水线，未规定大 schema 切批 | 预算器、社区切批、三遍生成、实体登记、连通性门禁（03） |
| [07 Gateway](../07-gateway.md) | 外部客户端 token：MCP 走 OAuth 2.1（1 小时可刷新），服务账号最长 90 天 | v0.2 先实现服务账号 token 的 renew/rotate/enroll、自助续期、到期监控与 `/v1/meta`（04 §5）；MCP OAuth 仍按 07 作为后续目标，不冲突 |
| [08 Agent 接口](../08-agent-interface.md) | `oo` 输出契约 | 新增 `oo login/whoami/share-prompt` 与管理端命令（04 §2） |
| [11 部署](../11-deployment-and-operations.md) | `./oo up` = `oo init` + standard 档 compose，要求 gVisor | 新增 lite 档作为开源默认（单所有者，不承诺多租户隔离）；standard 档不变（04 §4.2） |
| [12 配置参考](../12-configuration-reference.md) | 配置仓库字段 | 新增 `oo.stack/v1`（04 §4.1） |
| [13 评测](../13-evaluation-and-benchmarks.md) | OO-Logistics 生成器与题库 | 扩展为多引擎方言夹具与 20 道金标准题（05 §3） |
| [14 路线图](../14-roadmap-and-risks.md) | M0–M4 | v0.2 闭环与 sprint 见 Plan；M 里程碑定义不变 |
