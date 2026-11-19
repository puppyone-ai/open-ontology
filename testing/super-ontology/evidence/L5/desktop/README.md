# L5 Suggest 回流 · Desktop 会合证据（D80 阶段二，P9 轮，2026-11-18）

**状态：未签收（Desktop 部分未签）。**

状态口径（60 §3、30 §7.1）：**T1 通过**＝本次在当前树上对真实 `oo serve` 引擎跑过，日志在 `../../desktop-p9/logs/`；**T2 已写未跑**＝场景文件已写、`run-packaged.mjs --dry-run` 校验通过，但没有运行，因为这台 Mac 的终端缺少屏幕录制与辅助功能权限（`peekaboo permissions` 两项均 Not Granted）；**未做**＝既没有场景也没有运行证据。
**本文件不是签收。** `SO-SIGN-L<n>` 的 Desktop 会合由协调者在 T2 全绿、T3 提交、T4（该闭环要求时）完成后签；本文件只记录事实。运行环境、命令、缺陷清单见 `../../desktop-p9/README.md`。

30 §3 L5 的 Desktop 部分：`suggestion` 种类与当日摘要、修订模式批量接受、editor 角色、编辑回写、影响提示。

## 30 §3 L5 的 Desktop 验收项

| # | 验收项（Desktop 部分） | 状态 | 证据 / 说明 |
|---|---|---|---|
| 1 | 第 5 步“点接受”≤ 2 秒；六步跑通 | 接受耗时 T1 通过（假引擎）；六步全链 T2 已写未跑 | `logs/suggest-revision.log`：该 T1 用**假** `oo serve`（真文件夹）——当日摘要“4 条建议、3 条等你”、修订视图、一次接受后文件在磁盘上变化 138 ms、批量按来源接受、拒绝与撤销、冲突提示。引擎侧“下一次 BRIEF 变化”见 OO L5 a1 |
| 2 | 四类影响式规则 | 未做 | OO 侧项；Desktop 只展示（`ImpactHint`），无用例 |
| 3 | 审核负荷（50 条 ≤5 次操作） | 未做 | OO 侧项；Desktop 侧没有测量 |
| 4 | 每次接受可追溯到建议/凭证/证据 | 未做 | 回写走 `writeback.mjs`（原子改名 + `:sync`，Git 自动提交）；没有端到端用例 |
| – | `editor` 角色可选 | 未做 | 代码在（`RolePicker`）；没有用例 |

## 20 §7 的 T 卡

- F-S8-04：T1 suggest/revision-mode T1 通过（假引擎，`--native` 未跑）。
- T2 P17：T2 已写未跑。T3：未做。
