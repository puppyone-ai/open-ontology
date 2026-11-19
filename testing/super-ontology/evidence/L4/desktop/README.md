# L4 本体与共享 · Desktop 会合证据（D80 阶段二，P9 轮，2026-11-18）

**状态：未签收（Desktop 部分未签）。**

状态口径（60 §3、30 §7.1）：**T1 通过**＝本次在当前树上对真实 `oo serve` 引擎跑过，日志在 `../../desktop-p9/logs/`；**T2 已写未跑**＝场景文件已写、`run-packaged.mjs --dry-run` 校验通过，但没有运行，因为这台 Mac 的终端缺少屏幕录制与辅助功能权限（`peekaboo permissions` 两项均 Not Granted）；**未做**＝既没有场景也没有运行证据。
**本文件不是签收。** `SO-SIGN-L<n>` 的 Desktop 会合由协调者在 T2 全绿、T3 提交、T4（该闭环要求时）完成后签；本文件只记录事实。运行环境、命令、缺陷清单见 `../../desktop-p9/README.md`。

30 §3 L4 的 Desktop 部分：待确认队列、实体与纠错、模型横幅与用量、分享面板/一键执行/共享列表/撤销、漂移刷新、持续一致性、AIGC 标识。这一闭环要求 T4 owner 包。

## 30 §3 L4 的 Desktop 验收项

| # | 验收项（Desktop 部分） | 状态 | 证据 / 说明 |
|---|---|---|---|
| 1 | 配模型/不配模型的使用与横幅 | 配模型 T1 通过；无模型横幅 T2 已写未跑 | `logs/model-extract.log`：设置里配置模型（密钥落 0600 文件、引擎只持 `file:` 引用）、引擎调用回放模型 2 次、用量 `Today 2 / 2,000 calls`；P14 无模型 {T2} |
| 3 | BRIEF 每个 `[src:…]` 在 Desktop 里点开定位原文 | T1 通过 | `logs/share-mount-revoke.log`：BRIEF 有 10 个出处标记，每个都在 App 里打开到对应块 |
| 6 | 待确认队列：按来源整体接受一次操作 | T1 通过（回放模型，真实引擎） | `logs/review-accept.log`：按来源分组、整体接受后引擎队列为空、页面显示最新版本；冲突项没有整体接受入口。100 条规模的 ≤1 次操作与 5 秒入版本是 OO 侧（API 层） |
| 7 | Share 过期/撤销后列表显示正确 | 撤销 T1 通过；过期 未做 | share-mount-revoke：确认框写明“读过的内容无法收回”，引擎显示 revoked、`.context` 被删。过期后的列表状态没有 Desktop 侧用例 |
| 8 | 端到端第 1–3、6 步 | T1 通过（共享一步）；全链 T2 已写未跑 | 分享面板默认 suggester/7 天/800、预览 AI 生成标记、已拦截只给数量、命令含文件夹/范围/任务、一键执行与 CLI 的 42 个文件逐字节相同。P11 {T2} |
| 9 | 轻量漂移：5 秒内 `drifted`，刷新后旧的转 `refreshed` | T1 通过 | `logs/drift-refresh.log`：改动被引用文件后共享显示“需要刷新”并带计数、顶栏/侧栏有漂移计数、预览写明变化、刷新后旧共享 refreshed、新共享链回旧共享、新 BRIEF 头部带自上次以来的变化；耗时 4.7 s（含解析）。P18 {T2} |
| 10 | 合并/取代规则（当日摘要、`supersede_in_use` 提示） | 未做 | 代码在（`ImpactHint`、`reviewModel`）；没有 T1/T2 用例 |
| 11 | 持续一致性：新鲜度令牌、UPDATES.md 追加、`check` 返回 20、任务组整组刷新、`constraint_conflict`、用户纠错 | 未做 | 代码在（`ShareGroupRow`、`ConstraintConflictCard`、`FactCorrectionMenu`）；没有 T1 用例，P21 {T2} |
| 12 | P21 两个并行 Agent | T2 已写未跑 | `P21-parallel-agents.peekaboo.json`；没有运行 |
| – | P25 AIGC 标识与开源许可页 | T2 已写未跑 | 场景在；T1 share 断言了预览里的 AI 生成徽标 |
| – | P12 红队、P26 数据说明与出网 | **未做** | 场景文件不存在 |

## 20 §7 的 T 卡

- F-S6-06：T1 share/mount-and-revoke T1 通过、review/accept-by-source T1 通过，都是合成输入，`--native` 未跑；无模型各一遍：无模型版本 未做。
- T2 P1–P16（P12 缺）、P18–P21、P25、P26（缺）：T2 已写未跑。T3：未做。
- T4 owner 包：已构建（未签名），路径与 sha256 见 `../../desktop-p9/packs.md`；**应用启动在构建机上没能确认**（见同处已知问题 1）。
