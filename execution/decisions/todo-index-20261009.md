# 总台账 / TODO / 索引补齐回执

日期：2026-10-09。仅协调状态和文档工具，不新增产品功能验收。原产品里程碑、冻结分母和外来dirty文件保留。

## 交付

- [INDEX](../INDEX.md)：后续主智能体的紧凑阅读顺序和单一职责索引。
- [UPDATE-PROTOCOL](../UPDATE-PROTOCOL.md)：事件→必更文件表、完整小功能签收顺序和逐文件更新回执。
- [task-ledger](../task-ledger.json) / [TODO](../TODO.md)：完整父项覆盖，执行/交付/退出契约直接来自主Plan；父项历史签收尚待逐项对账，不表示未实现、不重置原开发进度。
- [生成器](../scripts/render_todo.py)：生成清单并检查漂移、父卡覆盖、Plan契约变化、无固定source/evidence/review的accepted；并未自动改Plan/CHANGELOG/CONTROL，也尚未接CI。
- CONTROL、运行快照、Handoff、Goal、active-work、开发总台账、计数附录、Plan、CHANGELOG、skill交叉链接到上述入口；未来对应事件须按协议更新。

## 验证

[TODO结果](../validation/todo-20261009/todo.log)：87 SP/14 DBX、16Phase/8DBP；清单与台账一致。

[拒绝变异](../validation/todo-20261009/mutations.log)：临时副本中四项无效状态被拒绝：重复父项、缺失DBX、无证据accepted、弱化Plan契约。不修改原任务状态。

[Spec](../validation/todo-20261009/spec.log)、[Plan](../validation/todo-20261009/plan.log)自测退出0；[skill格式](../validation/todo-20261009/skill.log)退出0。[引用检查](../validation/todo-20261009/links.log)与本次SHA清单供固定commit复核。

## 已知限制

当前SP父项状态needs_acceptance_reconciliation，须由主协调者逐固定代码、报告、独立review、完整退出条件核对；本轮没有把所有父项判为未开发，没有自动勾选任何SP/DBX。DBX开发分母须在派发前另行冻结。当前task-ledger是本次建立的父项/派发注册入口，原开发子项验收以计数附录和历史台账为准；不能将它替代原证据。

原先协调交接的SHA清单对应f834d6b5b固定commit。后续同名文档变化是正常协调更新；复核旧清单须用该固定commit，不能用现工作区新文件去判断旧证据是否完整。
