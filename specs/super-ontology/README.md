# Super Ontology Spec

状态：**Proposed**（2026-11-16 落版，待评审）。本目录不受 `specs/scripts/check_spec.py` 的哈希基线约束。

Super Ontology 是 Open Ontology 本轮的产品形态升级：在现有数据库流水线旁边，新增一条文档流水线，读懂 PDF、Word、Excel、PPT、Markdown 等文件，建出“人、项目、决定、别名”的本体，再按任务把一份带预算、逐行带出处、按范围物理隔离的上下文包（`.context/`）共享给 Agent。客户端承载在 Puppyone Desktop 的新页面“Super Drive”上，交互接近百度网盘。

| 分册 | 内容 | 读者 |
|---|---|---|
| [00 总纲与决策记录](00-overview-and-decisions.md) | 产品形态、边界、架构、决策 D1–D77、状态机、新鲜度、持续一致性、跨仓契约 | 所有人，先读 |
| [10 后端](10-backend.md) | OO 侧：provider、IR、流水线、抽取、可见性、Pack 编译、控制 API、鲁棒性、测试、`B-S*` 任务卡 | OO 工程师 |
| [20 前端](20-frontend.md) | Puppyone 侧：主进程服务、IPC、页面与交互、状态矩阵、设计规范落地、i18n、测试、`F-S*` 任务卡 | Desktop 工程师、设计 |
| [30 闭环与 Sprint](30-closed-loops-and-sprints.md) | 4 个里程碑、8 个闭环、14 个 Sprint、每个闭环的验收、横切门禁、时间表 | 所有人 |
| [40 开源借鉴](40-open-source-references.md) | 解析、本体、打包、写回、S3、网盘交互各自借了谁、为什么 | 评审、选型复盘 |
| [50 提交策略与追溯](50-commit-and-traceability.md) | ID 体系、分支模型、提交粒度、trailer、跨仓契约变更顺序、闭环签收与标签、台账、自动检查 | 所有提交代码的人 |
| [60 端到端测试与 owner 验收](60-e2e-and-owner-acceptance.md) | 测试方法调研、T1 源码真实点击 / T2 打包 App 黑盒 / T3 Agent 探索 / T4 owner 验收包、场景 P1–P18、验收清单模板 | 测试、前端、owner |
| [70 并行板块与排期](70-parallel-workstreams.md) | 12 个并行板块、目录所有权、契约先行与夹具解耦、关键路径、约 12 周排期、产能与同步节奏 | 主线程、所有执行者 |
| [80 形式化验证](80-formal-verification.md) | Quint 协议模型 FM1–FM9、Kani 证明 KP1–KP8、ITF 轨迹一致性测试、CI 作业、`V-S*` 任务卡、各闭环形式化门禁 | 后端、质量 |
| [90 商用边界](90-commercial-boundary.md) | 边界原则 C1–C10、组件准入表（一手核准）、SBOM 许可门禁、FFmpeg 与编解码专利、模型分发与署名、义务矩阵、AI 生成内容标识、律师门禁。用户须知由官网发布，不在本 spec 内 | 所有人、法务 |

阅读顺序：00 → 30 → 70 → 50 → 60 → 80 → 自己负责的 10 或 20 → 40。

改动规则：00 的决策记录（§4）和跨仓契约（§6）是唯一真相；改它们要先改 00，再同步分册，并在 PR 描述里写明影响了哪些任务卡。
