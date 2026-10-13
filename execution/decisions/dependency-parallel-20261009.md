# 五版交付的开发调度修订

日期：2026-10-09。依据：用户确认“可以 请修改”，同意保留五版顺序和十源门槛，修正全局开发等待规则。

规则入口：[产品闭环台账](../PRODUCT-LOOPS.md)。五版按顺序签收；后续独立任务可按真实固定依赖、独占文件及实际资源余量开发，当前交付关键路径优先。缺Oracle实例或许可不豁免DL01验收，也不阻塞无关开发。DL01先固化共用流程，逐个交付可用目标；必要测试、nightly和客户准备提前进行，DL05负责汇总补齐。

同步GOAL、HANDOFF、CONTROL及JSON、INDEX、UPDATE-PROTOCOL、结构化台账、TODO生成器、主Plan与事件记录。后续四版状态改名为waiting_for_delivery_signoff，明确等待的是整版签收，不是所有开发。原固定证据和历史决策保留，本修订覆盖其调度规则。

验收范围不变：80个必交SP父项、15阶段、14个DBX父项；SP15七项明确后续扩展保留。当前DL01不变，五版均未签收。未派发worker，未改变资源授权或暂停状态，未新增产品通过结论。

验证结果（均退出0）：

- [Spec完整性](../validation/dependency-parallel-20261009/spec.log)：8个损坏用例被拒绝。
- [Plan完整性](../validation/dependency-parallel-20261009/plan.log)：10个损坏用例被拒绝。
- [TODO一致性](../validation/dependency-parallel-20261009/todo.log)：注册及交付映射保持一致。
- [签收门槛](../validation/dependency-parallel-20261009/transitions.log)：私有副本上的10种不合法转态仍被拒绝，包括不足十源、缺Oracle、越序签收及未完成必交Plan。复现脚本为[transition-check.py](../validation/dependency-parallel-20261009/transition-check.py)。

这些检查只证明文档、台账及签收校验一致，不证明任何数据库或产品功能通过。并行开发的卡片依赖与资源条件由协调者派发前核对；本次没有新增自动调度引擎。
