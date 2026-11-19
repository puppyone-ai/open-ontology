# L7 企业与云 · Desktop 会合证据（D80 阶段二，P9 轮，2026-11-18）

**状态：未签收（Desktop 部分未签）。**

状态口径（60 §3、30 §7.1）：**T1 通过**＝本次在当前树上对真实 `oo serve` 引擎跑过，日志在 `../../desktop-p9/logs/`；**T2 已写未跑**＝场景文件已写、`run-packaged.mjs --dry-run` 校验通过，但没有运行，因为这台 Mac 的终端缺少屏幕录制与辅助功能权限（`peekaboo permissions` 两项均 Not Granted）；**未做**＝既没有场景也没有运行证据。
**本文件不是签收。** `SO-SIGN-L<n>` 的 Desktop 会合由协调者在 T2 全绿、T3 提交、T4（该闭环要求时）完成后签；本文件只记录事实。运行环境、命令、缺陷清单见 `../../desktop-p9/README.md`。

30 §3 L7 的 Desktop 部分：oss/cos/r2 登记与增改删、Cloud 代理模式、审计视图、远端挂载命令。真实桶需要 owner 自己的账号与密钥，这里不会创建账号。

## 30 §3 L7 的 Desktop 验收项

| # | 验收项（Desktop 部分） | 状态 | 证据 / 说明 |
|---|---|---|---|
| – | 三家对象存储“新增→修改→删除”界面正确 | T1 通过（假引擎） | `logs/enterprise-buckets.log`：27 项断言——每家对话框字段不同且不要求 endpoint、引擎收到的配置、密钥只落私有文件且不到引擎、列表标明种类/桶/只读/访问策略、对象 published→outdated→published→deleted。**真实桶：未做**。**缺陷 D1 同样适用**：按契约发的两个 `secret_ref` 真实引擎会 400，假引擎看不出来 |
| – | Cloud 代理模式切换、SSRF 白名单不放宽、无 Cloud 会话时提示登录 | T1 通过（部分） | `logs/cloud-proxy.log`：11 项——模式保存、数据说明随模式变、Cloud 模式下无会话的 Share 被拒且本地引擎与 Cloud 都没收到请求、本地模式照常。**未做**：真实 Cloud 会话下的 Share、真实项目的 Cloud 范围（报告 notRun） |
| – | 审计视图：谁何时把什么共享给哪个 Agent | T2 已写未跑 / 单测 | `P28-cloud-proxy` 含审计哈希链断言；`AuditView.tsx` 在树上 |
| – | 远端 Worker 的 Pack 与本地逐字节一致 | 未做（Desktop 侧） | OO 侧证据 `evidence/L7/l7-e2e-result.json`；Desktop 只生成远端挂载命令（命令不含令牌，P28 断言） |

## 20 §7 的 T 卡

- F-S12-03：T1 enterprise/buckets T1 通过、enterprise/cloud-proxy T1 通过（均假引擎/假 Cloud，`--native` 未跑）。
- T2 P27、P28：T2 已写未跑。T3：未做。
