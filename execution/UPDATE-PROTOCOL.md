# 功能闭环后的状态更新协议

更新：2026-10-09。负责人是主协调者Codex；worker只交付自己卡的源码/证据/资源回执，不自行更改总Plan、分母或别人的状态。此协议由 [工程索引](INDEX.md)、[Handoff](HANDOFF.md)、[skill](../.codex/skills/codex-grok/SKILL.md)、[TODO](TODO.md) 和 [CONTROL](CONTROL.md) 索引，作为每次交付之后的必经步骤。

## 1. 各文件唯一职责

| 文件 | 负责什么 | 禁止混用 |
|---|---|---|
| [产品交付闭环](PRODUCT-LOOPS.md) | 五次累计可用产品、当前DL、完整版本退出与签收证据 | 不将DL计入SP/DBX开发分母；交付按序签收；开发须固定依赖、独占范围及有实际资源余量 |
| [task-ledger.json](task-ledger.json) | 完整父项注册、当前派发/owner/固定依赖/路径、各验收层级、source/review/evidence引用、下一步 | 未对账不是未实现；父项不能由一个组件成功直接accepted |
| [TODO.md](TODO.md) | 上述台账生成的可阅读全量清单和父项checkbox | 不手工编辑勾选；不能把未勾父项当开发0% |
| [开发总台账](../specs/open-ontology-development-ledger.md) | 人类可读交付/修复/审阅/签收事件与历史依据 | 不只追加“最新”而放任顶部状态过时 |
| [验收计数](../specs/open-ontology-acceptance-counting.md) | 已冻结开发子单位、集成节点、发行包、返工口径与外部门槛 | 不完成后拆分抬高成功率；不合并87/14/90等分母 |
| [主Plan](../specs/open-ontology-v0.1-plan.md) | 原输入/输出/测试/退出定义；固定签收记录和对应父/Phase完成情况 | 不为完成率删要求；未达到全退出不能正式签收 |
| [CONTROL](CONTROL.md) / [control-state.json](control-state.json) | 当前运行/资源/授权快照、已知handle、就绪队列与汇总限制 | 不把OS进程或旧证据推断为当前live/资源已释放 |
| [active-work](active-work.md) | 派发/终态/资源移交事件历史及当前快照入口 | 历史live状态不能覆盖CONTROL；不同执行ID不混淆 |
| [CHANGELOG](CHANGELOG.md) | 具体行为变化、范围、source+证据/复审、验收等级、已知限制 | commit不等于review/integration/release全部通过 |
| [前端TODO入口](../specs/frontend/master-task.md) | 前端总卡里程碑及其关联TODO/验收 | 按总卡原位置更新；不重新派旧28卡或新开第四会话 |
| [数据库支持矩阵](../docs/database-support/support-matrix.yaml) | 逐目标逐能力支持等级；未来正式契约路径以DBX-W01为准 | 只有实际准入证据才晋级，不能从PG或协议兼容推导 |
| [GOAL](GOAL.md) / [HANDOFF](HANDOFF.md) | 目标/授权/退出与接续范式，目标范围或政策变更时维护 | 不因一次小功能完成改成whole goal complete |

原始结果在各task独立evidence目录；REPORT/FINDINGS/HANDOFF、raw exits、SHA清单和独立review须可定位。仓内状态引用它们，不重复复制客户数据、秘密、长原始日志。

## 2. 事件 → 必须更新的文件

“检查”意味着确认无需修改时留下本次回执的理由，不制造无意义diff。每次正式签收必查下面适用项，不能只更新一个文件。

| 事件 | 必更 | 按范围检查 / 更新 | 允许checkbox变化 |
|---|---|---|---|
| 冻结/派发 | task-ledger、CONTROL+JSON、active-work；生成TODO | requirements-map、执行prompt；新开发子项先在计数附录冻结 | 无完成勾选 |
| CLI退出/后台待收 | task-ledger运行状态、CONTROL+JSON、active-work | 原始phase exit/资源回执；writer后台未停不释放 | 无完成勾选 |
| 作者完整交付 | task-ledger的source/evidence与development状态、总台账、CHANGELOG、当前控制台；生成TODO | 精确commit、三报告、SHA、固定review队列；标作者完整交付，普通开发可先接续；独立审阅按最新review-policy在关键里程碑/联结闭环集中进行 | 不勾父项或集成/发行 |
| 固定独立review通过/退回 | task-ledger review及对应层级、总台账、CONTROL+JSON、CHANGELOG；生成TODO | 冻结开发子项达到原条件才更新计数；失败保留原因/修复owner/固定SHA | 仅对应已冻结且满足门槛的开发子项 |
| 产品集成闭环通过 | task-ledger integration、总台账、适用集成计数、CONTROL+JSON、CHANGELOG；生成TODO | 主Plan全部退出条目、Self-build/最小闭环/前端专项TODO、DBX目标准入（若适用） | 对应集成节点；父项全部退出才accepted |
| 父项 / Phase正式签收 | task-ledger parent状态、总台账、Plan固定签收引用、CONTROL+JSON、CHANGELOG；生成TODO | 完整REQ/AT/不变量/原型/依赖、对应专项TODO及计数口径 | 仅完整父项/Phase；一个子项不能勾整个Phase |
| 发行就绪/发布审批 | release层状态、发行计数与外部门槛、总台账、CONTROL+JSON、CHANGELOG | 固定制品、双架构、部署恢复、签名/CI/runsc/客户条件；发布仍需对应授权 | 仅实际通过的发行包，非自动执行发布 |
| 用户优先级/授权/范围改变 | GOAL、HANDOFF、CONTROL+JSON、task-ledger、主Plan执行附录、CHANGELOG；生成TODO | INDEX/skill规则和专题总卡；如改规范走ADR/source-baseline复核 | 不因排期变化勾完成 |

开发、集成、发行各自记录，外部门槛单列。DBX实验能力开发通过与verified/certified晋级是两个事件；14天nightly等未满足不能晋级。旧平台Goal paused并不由仓内状态更新自动恢复。

产品整版交付还必须更新PRODUCT-LOOPS总checkbox/签收表、task-ledger中的delivery_loops及current_delivery_loop、CONTROL/JSON、Goal接续位置与CHANGELOG。先核本版全部条件、对应最迟签收的SP/DBX父项及阶段和回归，正式签收后才切下一DL；DL05另核全部80必交SP/14DBX和15阶段正式签收，任务/父项的局部批准不触发切换。current_delivery_loop指当前整版验收目标，不是开发范围上限；后续独立任务注明自己的所属DL、固定实现/接口SHA、文件与资源边界，经协调者排入台账后可开发。局部完成照常入账，不提前勾后续整版；不抢当前关键路径，不用stub代替真实依赖。

## 3. 一次“小功能闭环”签收的操作顺序

1. **核身份与范围**：task/Plan ID、REQ/AT、真实输入/接口/下游、固定source/deps SHA；读取全部要求，不依赖worker一句“完成”。
2. **核决定性证据**：原始退出码、实际binary/fixture/model身份、typed oracle与审计、必要失败无副作用、资源终态；验证SHA清单。缺任一适用门槛标待证/返工，保留失败。
3. **判定审阅时点**：按2026-10-10用户要求及review-policy.json，普通功能作者完整固定交付可先接续，记录review_deferred_to_integration；不冒充独立批准。协调者判为关键里程碑或验证产品/联结闭环时核对应固定SHA独立review；源码变化不继承旧批准。
4. **判验收层级**：是作者交付、开发子项通过、集成闭环通过还是父项/Phase退出？记录限定范围。若只是最小草案成功，保留正式审核/分类/发布待办。
5. **更新task-ledger**：真实owner/session/handle、路径/资源、source/evidence/review、development/integration/release状态、next_action；parent只有全部退出才accepted。当前字段不够时受控扩展schema，不能把猜测填成事实。
6. **更新总台账与计数**：写事件/范围/source/review/原始证据路径；只对原冻结子项转态。返工前置缺契约与review修复分别记录，修复/补报告不增加功能分母。
7. **更新Plan及专项TODO**：父项未完保留未勾；记录哪条退出已证、剩余owner/条件。前端更新master-task所指定TODO；DBX准入只在等级全部满足时更新正式矩阵。
8. **更新CONTROL/JSON与active-work**：真实退出、资源交接/释放/保留、下一批ready依赖；未收后台writer保持独占，不明进程保留unknown。
9. **更新CHANGELOG**：用户可观测行为变化、固定source/evidence/review、验收等级、限制，勿写“全完成”概括局部能力。
10. **生成与校验TODO**：`python3 execution/scripts/render_todo.py`、`--check`；如Spec/Plan涉及，跑文档完整性检查；核所有新引用及精确staged diff。
11. **提交协调状态变更**：只提交本次拥有的协调路径，记录父commit/工作回执；产品代码commit与协调签收commit分开引用。复用稳定模板不复用旧run结果。
12. **继续下一项**：已核实释放资源后推进独立ready卡；未满足整体Goal退出就保留未完，不因为小闭环、时间压力或未知外部条件结束整体目标。

## 4. 更新回执模板

每次事件写在总台账/CHANGELOG或本task HANDOFF，避免多处复制整报告：

```text
Event/task/Plan IDs：
Capability + acceptance level + limitations：
Source/deps/binary/fixture hashes：
Author packet + independent review + decisive raw result paths：
Updated task-ledger / TODO：
Updated development ledger / counting scope and numerator delta：
Plan/specialized TODO：updated or no parent exit yet (reason)
Updated CONTROL/JSON / active-work / resource receipt：
Updated CHANGELOG：
DB support matrix / frontend TODO / GOAL：applicable change or explicit N/A
Verification commands + actual exits：
Coordination commit / remaining owner / next action：
```

历史证据清单中的总台账/Plan等可变文档hash属于**当次固定commit**。后续更新不篡改原清单；复核旧文档必须取原commit，不能拿未来工作区内容对旧hash校验再称原证据损坏。

## 5. 自动化边界

已实现：结构化台账覆盖验证、无必要固定证据的parent accepted拒绝、TODO生成/漂移检查。**尚未实现**：自动从产品测试结果完成全部验收、自动改Plan/CHANGELOG/运行控制台、已有父项历史证据的全量对账。这些仍由协调者依据本协议执行，不宣称存在自动联动服务或已接入CI。若以后接入CI，独占writer和secret排除仍适用。

### 2026-10-10 可靠并发与等待更新

用户要求可靠执行者并发15，普通事件等待900秒（15分钟），退出立即收集；旧600秒为历史。Rust实际global10/task1，Node2/browser2、默认PGtask1及明确源目标例外、Docker16CPU及内存headroom保持。15独占执行者共享实际构建资源池，待依赖/待资源与实际运行区分，不把排队当构建。

### 2026-10-10 最新用户闭环交付与等待要求

每数据源唯一自主工程师负责完整产品链；重复driver/consumer工程师合并，代码/失败/资源归属保留。每次工作收集真实完整交付，不用小功能状态和CLI数量替代闭环。固定等待调用clock.sleep900秒（可被新输入中断），保持本轮工作，不每分钟结束对话。仍按原10源含MySQLOracle和Plan全部退出验收；SQLServer真实环境缺口不凑数。

### 2026-10-12 人类 Ponytail 与调用成本约束

使用用户指定 ponytail full：最小完整改动、复用现有代码/固定测试、禁止过度工程；执行细则见 batches/native-full-source-20261012/PONYTAIL.md。子智能体总 Token 目标为可比此前 <=1/5；当前缺逐子 usage/硬限额接口，达标未验证，不宣称保证或通过。依赖无独立工作时一次交接后停止等待型采样，就绪固定制品到达再续跑；真实完整十源退出/必要安全与测试不降低，可靠并发最多15有用工程师，不用空转凑数。

### 2026-10-12 人类临时备用模型授权

多个原生工程师实际返回 Selected model is at capacity 后，人类明确回复“暂用 GPT-6 Sol low”。仅停止owner的原完整任务移交原生gpt-6-sol/low备用工程师，源码/固定证据/具名计算与资源保留，先核终态再收结果，不重复writer或新增分母。仍运行的Maria工程师不重复派发。产品Selfbuild/对话仍gpt-6.1-sol/low、URL/key不变；Ponytail最小完整改动与Token20%实测unknown、十源全部原退出及统一闭环审阅条件保持。

### 2026-10-12 共享稳定优先

最新人类要求覆盖此前最大并行排期：共享项目真实完整集成稳定后，才恢复十源完整任务。execution/shared-readiness-gate.json 是放行依据，固定源码/制品/真实同版本参考及原始失败证据；root 判所有退出成立才放行，作者组件测试不等于通过。仅必要只读参考输入交接允许进行，不计源验收或增加分母。普通等待900秒；历史限额/旧运行快照不可当当前授权。
