2026-10-15 MySQL完整参考链已移交唯一`gpt-6.1-sol/medium`负责人`/root/mysql_shared_reference_sol`。原Luna xhigh已完整交接并经实际list_agents确认completed，root核producer/pipeline/worker/manifest四SHA匹配、MySQL构建容器仅sleep后才新派发；未丢WIP/失败、无重复writer。真实五表typed导入仍为拒绝占位，直接推进最小正式桥及原完整C1同步/S0–S10/维护Pack/正式发布/Gateway/随机Agent退出。合同MYSQL-SOL-COMPLETE.md、mysql-sol-handoff.json及mysql-reference-dispatch.json位于execution/batches/native-luna-full-source-20261015。无完整作者交付/整源签收，0/10与其余九源held不变；未增加开发/集成/发行或外部门槛计数，旧构建专属exit receipt缺失明确保留。

# 工程执行索引

更新：2026-10-09。产品交付是Ontology生产系统与可外接的产物/接口，内置Pi为体验及内部验证。定义见[来源revision4](decisions/source-baseline-revision-4.md)。后续主智能体先读本页与最新用户消息，再读对应功能要求；不需要加载整段会话或历史台账。

| 阅读顺序 | 入口 | 要解决的问题 |
|---|---|---|
| 1 | [GOAL](GOAL.md) | 完整范围、优先级、授权与退出条件 |
| 交付路线 | [产品闭环台账](PRODUCT-LOOPS.md) | 五次可用产品交付、十源要求、按序签收及按依赖并行开发的条件 |
| 2 | [CONTROL](CONTROL.md) / [运行快照](control-state.json) | 当前任务/资源事实、未知归属、等待与并发限制 |
| 3 | [TODO](TODO.md) / [结构化任务台账](task-ledger.json) | 87 SP / 14 DBX / 8 DBP的全量覆盖，哪些待对账/派发/正式签收 |
| 4 | [状态更新协议](UPDATE-PROTOCOL.md) | 每次派发、交付、复审、集成或发行要更新哪些文件、何时允许勾选 |
| 5 | [HANDOFF](HANDOFF.md) / [项目skill](../.codex/skills/codex-grok/SKILL.md) | 任务拆分、CLI生命周期、固定审阅、证据与资源交接 |
| 6 | [主Plan](../specs/open-ontology-v0.1-plan.md) / [Spec](../specs/open-ontology-v0.1-spec.md) / [docs](../docs/README.md) | 本卡原定输入/接口/测试/退出，不能用TODO摘要取代 |
| 7 | [数据库扩展](../docs/database-support/README.md) / [前端总卡](../specs/frontend/master-task.md) | 专项契约、准入和固定会话约束 |
| 8 | [开发历史总台账](../specs/open-ontology-development-ledger.md) / [验收计数](../specs/open-ontology-acceptance-counting.md) | 已交付组件、冻结分母、原始事件和证据 |
| 9 | [CHANGELOG](CHANGELOG.md) / [active-work历史](active-work.md) | 变更摘要及事件溯源，不能把旧句柄表当live状态 |

**一次功能闭环后的必读入口是 UPDATE-PROTOCOL，不是凭记忆只改 CHANGELOG。** 任务台账存当前父项和派发信息；冻结计数存开发子项验收；CONTROL存运行事实；TODO由台账生成；Plan定义父项门槛。它们的职责不同，不互相替代。

校验入口：`python3 execution/scripts/render_todo.py --check`，以及 `python3 specs/scripts/check_spec.py --self-test` / `python3 specs/scripts/check_plan.py --self-test`。这些命令只证明文档/台账一致性，不证明产品闭环通过。

当前审阅时点见 [review-policy](review-policy.json)：2026-10-10直接人类授权，普通功能不强制逐项独立review，完整作者交付可先联结，关键里程碑和闭环集中独立审阅。

2026-10-10最新排程：可靠执行者15；普通事件等待900秒，退出立即验收；实际Rust10/task1与CPU16不放宽。

## 2026-10-12 最新人类编译与CPU授权

直接人类要求解除全部编译槽和CPU限制。旧Rustglobal10/task1、CPU16预留总额、builder --cpus1、CARGO_BUILD_JOBS=1及按固定槽排队规定已被覆盖。后续构建不设置固定并行jobs/CPU配额/编译槽上限，使用当前机器可用CPU与内存；独占源码/固定archives、源只读预算、真实测试及权限边界保持。容器CPU按实际cgroup无quota验证；正在运行的编译不中断，旧进程启动时jobs环境只能在下一构建调整，不把文档变更冒充运行参数已改变。可靠Grok目标15与普通clock.sleep900秒保持，worker遇旧资源条文以本最新人类授权为准；依据当前真实机器状态安排，不按旧固定CPU/槽阈值阻塞。

### 2026-10-12 最新直接人类：原生GPT执行与统一闭环审阅

Grok额度不足，后续开发切换Codex原生子智能体，统一gpt-6.1-sol/low，禁止新增Grok CLI。原15个CLI均已终态、已核0个原UUID进程，具名后台计算/数据库保留交接。十源完整产品链全部继续，MySQL/Oracle重点；每源唯一自主工程师，输入/接口/独占写域/真实测试与量化全部退出明确，无独立组件审阅子任务。仅首轮Goal完整闭环及十源完整导入/Selfbuild/同步/Ontology维护/Pack/外部接口/Gateway/实际对话具备后统一独立审阅，普通小问题延后；作者自验与实际证据不可省、未跑不能通过。对话Agent模型gpt-6.1-sol/low，现有代理URL和key不变，密钥仍私有引用。新增Oracle47.104.150.9:1521/scm5仅真实只读选表，复用用户测试secret私有文件，连接身份按实际验证，不枚举秘密。普通clock.sleep900秒、可靠并发15、编译/CPU固定上限解除保持。验收计数不变，整源0/10，Goal保持active。
