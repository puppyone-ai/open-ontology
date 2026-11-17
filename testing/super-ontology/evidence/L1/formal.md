# L1 形式化门禁（80 §8：工具链锁定；FM1 草稿评审通过）

| 项 | 结果 | 依据 |
|---|---|---|
| 工具链锁定 | 通过 | `formal/TOOLCHAIN.md`：Quint 0.33.1、Apalache 0.62.4（jar sha256 记录）、Kani 0.68.0、OpenJDK 21；均为开发与 CI 工具，不进产物；`supply-chain/` 已登记；CI `formal.yml`（typecheck、simulate、按不变式分片的 verify-fast、conformance） |
| FM1 草稿评审 | 通过（带修改，修改已落地） | `formal/reviews/FM1.md`：建模者自审 + 独立原生子智能体评审（APPROVE WITH CHANGES，2 项阻塞）+ 实现侧负责人裁定；两项阻塞均已修复：解析作业到上限一律隔离（迁移 0138，按 10 §4.1.6），新增 `invCancelProvenance` |

## FM1 当前状态（模型 commit 5dce12c37）

- 边界：3 worker、4 作业，verify 深度 10；10 个不变式逐个 `quint verify`（Apalache）无反例。
- 随机模拟：每模型 10,000 条轨迹，固定种子，无违规。
- 一致性：10,000 条生成轨迹（30 万步）对真实 SQL 函数逐步比对一致；修复前的 SQL 会被该测试判失败（测试有判别力）。
- 未覆盖：活性（未装 TLC，L4 前的 `formal-verify-deep` 处理）；checkpoint 单调性与回收公平性（评审非阻塞项，L3 跟进）。
- 已知风险：`invNoLostJob` 单项约 30 分钟，超出 CI 每项 15 分钟预算；L2 门禁前改为 fast/deep 两档边界。
