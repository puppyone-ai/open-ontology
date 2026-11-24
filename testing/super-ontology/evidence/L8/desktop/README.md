# L8 规模与轻量 · Desktop 会合证据（D80 阶段二，P9 轮，2026-11-18）

**状态：未签收（Desktop 部分未签）。**

状态口径（60 §3、30 §7.1）：**T1 通过**＝本次在当前树上对真实 `oo serve` 引擎跑过，日志在 `../../desktop-p9/logs/`；**T2 已写未跑**＝场景文件已写、`run-packaged.mjs --dry-run` 校验通过，但没有运行，因为这台 Mac 的终端缺少屏幕录制与辅助功能权限（`peekaboo permissions` 两项均 Not Granted）；**未做**＝既没有场景也没有运行证据。
**本文件不是签收。** `SO-SIGN-L<n>` 的 Desktop 会合由协调者在 T2 全绿、T3 提交、T4（该闭环要求时）完成后签；本文件只记录事实。运行环境、命令、缺陷清单见 `../../desktop-p9/README.md`。

30 §3 L8 的 Desktop 部分：1 万文件首屏与滚动、向量状态与相关标记、免 Docker 运行时引导。这一闭环要求 T4 owner 包。

## 30 §3 L8 的 Desktop 验收项

| # | 验收项（Desktop 部分） | 状态 | 证据 / 说明 |
|---|---|---|---|
| 1 | 1 万文件 Drive 首屏 < 1 秒，虚拟滚动无掉帧 | T1 通过（有一次越线） | `logs/scale-10k.log`：首屏 154 ms，但“向下滚动并加载分页”p95 17.0 ms（门限 16.7）**失败**；重跑两次 `logs/scale-10k-rerun1.log`、`scale-10k-rerun2.log` 通过（p95 10.4、10.0 ms，最大 25 ms）。结论：本机有其他负载时会越线，稳定性未证明。该 T1 用假引擎。P30 {T2} |
| 2 | 冷启动建 Index 的时间与内存 | 未做（Desktop 侧） | OO 侧 `evidence/L8/bench-scale10k.*` |
| 3 | 无 Docker 的 macOS 机器从安装到第一份 BRIEF ≤ 10 分钟 | 引导页 T1 通过（假运行时）；整条路径 T2 已写未跑/未做 | `logs/docker-free.log`：不含 Docker 的检查与字样、以 `oo serve --runtime host` 启动、引擎页写明内置数据库与解析边车。**未做**：真实内嵌 PostgreSQL、打包 App 上的 ≤10 分钟（P29 {T2}）；这台机器装有 Docker，无法在此验证“没有 Docker 的机器” |
| – | 向量状态行与相关标记 | 未做 | 代码在（`VectorStatusRow`、docs-api 1.9 `vector`）；没有用例 |

## 20 §7 的 T 卡

- F-S14-02：T1 scale/ten-thousand T1 通过（一次失败、两次重跑通过）、runtime/docker-free T1 通过。
- T2 全量（含 `--drive-size 10000`）：T2 已写未跑。T3：未做。
- T4 owner 包：已构建（未签名），含 `sample-drive-10k/` 与 L8 追加清单 26–30，见 `../../desktop-p9/packs.md`。**应用启动在构建机上没能确认。**
