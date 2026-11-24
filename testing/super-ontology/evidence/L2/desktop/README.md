# L2 行走骨架 · Desktop 会合证据（D80 阶段二，P9 轮，2026-11-18）

**状态：未签收（Desktop 部分未签）。**

状态口径（60 §3、30 §7.1）：**T1 通过**＝本次在当前树上对真实 `oo serve` 引擎跑过，日志在 `../../desktop-p9/logs/`；**T2 已写未跑**＝场景文件已写、`run-packaged.mjs --dry-run` 校验通过，但没有运行，因为这台 Mac 的终端缺少屏幕录制与辅助功能权限（`peekaboo permissions` 两项均 Not Granted）；**未做**＝既没有场景也没有运行证据。
**本文件不是签收。** `SO-SIGN-L<n>` 的 Desktop 会合由协调者在 T2 全绿、T3 提交、T4（该闭环要求时）完成后签；本文件只记录事实。运行环境、命令、缺陷清单见 `../../desktop-p9/README.md`。

30 §3 L2 的 Desktop 部分：引擎生命周期与登记、状态实时更新、断线重连、新鲜度、本地数据说明。F-S2-01..08、10..13 已在 `codex/super-drive`，F-S2-09（T2）只有场景文件。

## 30 §3 L2 的 Desktop 验收项

| # | 验收项（Desktop 部分） | 状态 | 证据 / 说明 |
|---|---|---|---|
| 1 | 登记含 20 个 Markdown 的文件夹，60 秒内全部 `published` | T1 通过（4.5 s） | `logs/skeleton.log`：A1 列表 20 个 published、引擎一致；T2 P2/P3/P4(md) T2 已写未跑 |
| 2/3 | `oo context mount` 与逐字节相同（OO 侧）；Desktop 的“复制命令”路径（P11 复制命令） | OO 侧已签；Desktop：T2 已写未跑 | P11 `P11-share-and-mount` 是 L4 版（含一键执行），L2 版“只复制命令”没有单独场景；一键执行与 CLI 逐字节相同见 L4 |
| 4 | 关 `oo serve`：界面显示“引擎已断开”、自动重连、`since` 补齐不丢状态 | T1 通过（部分） | `logs/skeleton.log`：A4 界面显示 restarting、6.1 s 内用新守护进程重连、20 个文件状态不变、事件头 151→165、断连期间新增文件到 published。410 全量重拉：只有单元测试（vitest `tests/unit/super-drive/events`，44 个文件 540 条通过），没有活体证据 |
| 5 | 改一个 Markdown，30 秒内重走状态 | T1 通过 | A5：在磁盘上改文件（不经 App）后 published→queued→parsed→indexed→published 4.5 s；BRIEF 内容变化见 OO L2 a5 |
| 6 | 顶栏新鲜度在 `up_to_date`/`processing`/`publishing` 间切换并显示版本 | T1 通过 | A6：up_to_date→processing→publishing→syncing→up_to_date，文字 `Up to date · v1 · now` |
| 7 | 底部常驻“数据在本地”说明；默认状态抓包除 loopback 外无出网 | 说明文字：T1 通过；抓包：未做 | D79 行在 skeleton 首次页断言；**P26（出网抓包）场景文件不存在**，没有抓包证据。OO 侧 `evidence/L2/a7-sockets.json` 只覆盖引擎进程 |

## 20 §7 的 T 卡

- F-S2-01..08、10..13：代码在树上，T1 skeleton T1 通过（15 项断言全过）。
- F-S2-09 T2（P2、P3、P4 md、P8、P11）：场景文件存在，T2 已写未跑。
- A1–A5 可访问名称覆盖率：架构测试随 vitest 通过（`logs/vitest-super-drive.log`）；Peekaboo 侧按名称点击未验证（T2 已写未跑）。
