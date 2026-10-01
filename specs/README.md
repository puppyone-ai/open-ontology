# Open Ontology 实施规范

主规范：[Open Ontology v0.1 产品与工程实施 Spec](open-ontology-v0.1-spec.md)。

执行计划：[Open Ontology v0.1 开发执行 Plan](open-ontology-v0.1-plan.md)。按“契约驱动、风险先证、证据闭环”组织，逐项细化87个工作项与P1–P16，供工程师和编程Agent共用；包含任务模板、PR与审阅规则、CI激活、证据签收及首批执行清单。

该规范以当前 `docs/` 的全部 19 份文档为输入，定义产品边界、Rust 实施契约、安全控制、数据与接口模型、16 个可拆解的 Sprint Phase、测试矩阵、审阅要求、CI/CD 与客户评测闭环。

状态为 **Proposed / Ready for implementation review**。规范中的产品命令、测试入口和交付物是实现的验收要求；已有代码的完成状态须由执行台账及实际验证证据确认，不能据此声称产品已经构建、测试或交付。

建议阅读顺序：主规范第 1–5 章 → 第 17 章 Sprint Phase → 对应组件契约 → 第 18–21 章测试、审阅与发布 → 第 22–24 章追踪矩阵、风险与来源。

开始实施时先读Plan第1–3章工作协议，再领取第4章执行卡，并回到Spec相应契约/验收。文档核查：`python3 specs/scripts/check_spec.py --self-test`、`python3 specs/scripts/check_plan.py --self-test`；两者均不证明产品已实现或运行通过。
