# L3 读懂文件 · Desktop 会合证据（D80 阶段二，P9 轮，2026-11-18）

**状态：未签收（Desktop 部分未签）。**

状态口径（60 §3、30 §7.1）：**T1 通过**＝本次在当前树上对真实 `oo serve` 引擎跑过，日志在 `../../desktop-p9/logs/`；**T2 已写未跑**＝场景文件已写、`run-packaged.mjs --dry-run` 校验通过，但没有运行，因为这台 Mac 的终端缺少屏幕录制与辅助功能权限（`peekaboo permissions` 两项均 Not Granted）；**未做**＝既没有场景也没有运行证据。
**本文件不是签收。** `SO-SIGN-L<n>` 的 Desktop 会合由协调者在 T2 全绿、T3 提交、T4（该闭环要求时）完成后签；本文件只记录事实。运行环境、命令、缺陷清单见 `../../desktop-p9/README.md`。

30 §3 L3 的 Desktop 部分：上传与队列、状态矩阵、失败横幅/筛选/批量重试、详情与锚点、s3 登记、虚拟滚动、自动感知、新鲜度七态。

## 30 §3 L3 的 Desktop 验收项

| # | 验收项（Desktop 部分） | 状态 | 证据 / 说明 |
|---|---|---|---|
| 1 | 每个失败文件都有可读错误码与处理建议 | T1 通过（部分） | `logs/formats-failures.log`：不支持、过大、加密、旧版 doc 各进入正确终态并在列表里显示；横幅文案按 8 语言的校验在 `check:localization`（check:boundaries 通过） |
| 2 | 故障注入：解析中杀掉 sidecar，其余文件继续、界面显示降级 | **T1 失败** | `logs/parser-outage.log`：杀掉 parser 容器后等不到“降级”。**缺陷 D2**：OO `/health` 固定把 parser 记为 ok（`crates/oo-docs/src/health.rs:238`），“解析器不可用”信号在 OO 里不存在，Desktop 的 `Waiting: Parser unavailable` 没有来源。PG 断开、对象存储 5xx 等是 OO 侧项 |
| 5 | fs 与 s3 各跑“新增→修改→重命名→删除” | fs：T1 通过（新增/修改见 skeleton，上传见 formats）；s3：**T1 失败** | `logs/s3-incremental.log`：在对话框里登记 RustFS 桶时引擎返回 400。**缺陷 D1**：桌面端按契约发 `access_key_id`/`secret_access_key` 两个 `secret_ref`，引擎 `s3_config` 只收单个 `secret_ref`（`crates/oo-docs/src/api/drives.rs`）。OO 自己的 s3 矩阵用的是单个 `secret_ref`，所以 OO 侧是绿的 |
| 7 | 每个文件状态都有呈现（逐项截图）；断线重连不丢状态 | T1 通过 | `logs/state-matrix.log`：每个状态行亮暗两套截图并断言文字，新鲜度栏各状态；重连见 L2 4 |
| 8 | 自动感知（访达里放/改/删）；D49 单文件延迟 | 部分；P19/P20 未做 | skeleton A5：磁盘上改 md 后 4.5 s 重新发布（监听 D46）。访达里新增/删除、PDF 20 页 ≤30 s、XLSX ≤20 s 没有 Desktop 侧测量；**P19、P20 场景文件不存在** |
| 9 | 定向同步不触发全量清扫；新鲜度七态全部可见 | 七态：T1 通过；不全量清扫：未做 | 七态见 state-matrix；`:sync` 带 `paths` 的 OO 侧证据在 L3；Desktop 侧无负向用例 |
| – | 详情“解析结果”标签与锚点定位 | T1 通过 | `logs/details-anchor.log`：6 项断言，块列表带源码行号，定位高亮原文片段（T2 P10 {T2}） |
| – | 上传（拖拽/多选/右键/横幅→筛选→批量重试） | 合成输入 T1 通过；原生点击 未做 | formats-failures：多选、右键菜单、上传服务接受拖入路径→published。`--native` 需要辅助功能权限，`logs/native-flag-on-entry.log` 记录了失败（Timed out waiting for native window focus） |

## 20 §7 的 T 卡

- F-S4-05 T1：processing/formats-and-failures T1 通过；processing/parser-outage **失败**；sync/s3-incremental **失败**（D1、D2）。
- F-S4-05 T2：P1–P8、P10、P15、P16 场景文件在，T2 已写未跑。
- T3 任务描述 `tests/support/super-drive/t3-task-L3.md` 在树上，T3 未做。
- 未写的 L3 T2 场景：P19、P20。

代码状态（从树上重新推导，旧 gap 表过期）：F-S3-05 网格视图（DriveFileGrid）、F-S3-07 监听（`drive-watcher.mjs`）、F-S3-08 新鲜度七态与目录汇总、F-S4-01..04 均有代码与单元测试；是否在真实流程里正确，见上表。
