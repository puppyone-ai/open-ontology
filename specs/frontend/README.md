# Open Ontology 前端体系索引

| 属性 | 值 |
|---|---|
| 文档包 | OO-FE-001 / v1.0 |
| 日期 | 2026-10-08，Asia/Shanghai |
| 状态 | 实施设计基线；尚未开始本包的代码交付或产品验收 |
| 当前来源快照 | c23db28dc27f5cf20741dd36e8a8e88129ad1715；主进程后续提交不改变本包来源记录 |
| 用户决定 | `/` 用户对话入口，`/admin` 管理入口；分别登录与校验权限，共享 UI/UX 规范 |
| 上位要求 | [主 Spec](../open-ontology-v0.1-spec.md)、[完整 Plan](../open-ontology-v0.1-plan.md) |

## 阅读与实施顺序

1. [设计系统与产品体验总规范](design-system.md)：全产品范围、审美、双入口、Token、内容、组件和工程边界。
2. [内容体系与业务表达](content-system.md)：来源层、术语、专家问题、错误与反馈。
3. [页面与用户旅程规格](page-specifications.md)：页面索引、路由、对象、动作、布局、状态和响应式。
4. [公共组件契约](component-contracts.md)：交互、键盘、显示及组件复用规范。
5. [API 与状态契约](api-state-contracts.md)：现有协议引用、待绑定的管理操作、身份、版本、流式与精确值。
6. [实施任务与依赖索引](delivery-plan.md)：任务卡、目录归属、依赖、后端绑定与范围追踪。
7. [任务卡公共协议](task-protocol.md)：每卡继承的开发、验证、提交和审阅规则。
8. [验收与证据规范](acceptance-evidence.md)：功能、视觉、接口、真实端到端、CI/CD 与发行条件。
9. [覆盖与来源矩阵](coverage-sources.md)：关联主 Plan/Spec、skill 与官方实践。
10. [来源内容哈希](source-baseline.md)：记录读取的主规范/Plan及docs文件版本。
11. [任务卡目录](tasks/README.md)：只领取依赖就绪的卡；卡内只写差异。

## 权威与冲突处理

Spec/Plan 的产品、安全、权限、数据、状态、发行要求优先。本包细化前端，不替换后端契约。用户明确决定的双入口覆盖此前讨论中的 `/console` 与 `/workspace` 建议；本包只用 `/` 和 `/admin`。未来变更必须记录版本、受影响页面/组件/测试，不能在任务卡里私改公共规则。

文档责任：design-system 是视觉/内容真相源；page-specifications 是路由/页面真相源；component-contracts 是共享交互真相源；api-state-contracts 是绑定规则真相源（正式字段仍以发布 OpenAPI/Schema 为准）；acceptance-evidence 是前端验收真相源；tasks 只定义各任务写入差异。缺少后端能力登记 dependency，不退化为本地权限或假发布。

所有 FE 编号是现有 SP 工作项的分解映射；不增加主 Plan 父项，不擅改原冻结验收分母，不把本包成文记作产品完成。各任务当前均 `planned`。本侧会话只交付文档，未启动任何 Grok 或其他智能体任务。
