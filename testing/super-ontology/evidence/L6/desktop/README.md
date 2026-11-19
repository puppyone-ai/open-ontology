# L6 多模态与演进 · Desktop 会合证据（D80 阶段二，P9 轮，2026-11-18）

**状态：未签收（Desktop 部分未签）。**

状态口径（60 §3、30 §7.1）：**T1 通过**＝本次在当前树上对真实 `oo serve` 引擎跑过，日志在 `../../desktop-p9/logs/`；**T2 已写未跑**＝场景文件已写、`run-packaged.mjs --dry-run` 校验通过，但没有运行，因为这台 Mac 的终端缺少屏幕录制与辅助功能权限（`peekaboo permissions` 两项均 Not Granted）；**未做**＝既没有场景也没有运行证据。
**本文件不是签收。** `SO-SIGN-L<n>` 的 Desktop 会合由协调者在 T2 全绿、T3 提交、T4（该闭环要求时）完成后签；本文件只记录事实。运行环境、命令、缺陷清单见 `../../desktop-p9/README.md`。

30 §3 L6 的 Desktop 部分：OCR 与图片呈现、类型升格卡、差异预览与 `reprocess_diff`、宿主机媒体解码、说话人确认、媒体呈现、模型。这一闭环要求 T4 owner 包。

## 30 §3 L6 的 Desktop 验收项

| # | 验收项（Desktop 部分） | 状态 | 证据 / 说明 |
|---|---|---|---|
| 1 | OCR 低置信区域标记、类型升格卡片 | 未做 | 代码在（`BlocksTab`、`TypePromotionCard`）；没有 T1/T2 用例，P23-P25 只覆盖媒体与 AIGC |
| 4 | 录屏关键帧：每页一帧、帧文字可搜、`#t=`/`#f=` 跳转 | T2 已写未跑 | P24、P22；代码 `KeyframeStrip`/`MediaPlayer`；没有运行 |
| 5 | macOS 上 H.264/AAC 经宿主机解码；无解码器时 `DOCS_PARSE_CODEC_UNAVAILABLE` 与“在此 Mac 上解码” | 部分 T1 通过 | `logs/media-decode.log`：m4a/mp4 放进 Drive 后行上显示媒体队列、如实显示引擎状态；**本运行的引擎没有请求宿主机解码（没有 media-parser sidecar），codec 卡步骤没有走到**（报告 notes）。P22 {T2} |
| 3 | 说话人确认：确认前内容不入本体、界面没有声纹库入口 | T2 已写未跑 | P23；代码 `SpeakerIdentityCard`/`SpeakerPanel`；没有运行 |
| 7 | 改被引用的决定，刷新后 BRIEF 头部列出差异 | T1 通过 | 同 L4 9：`logs/drift-refresh.log` 新 BRIEF 977 字节，含自上次以来的变化 |
| 8 | 解析器升级：`outdated`、`reprocess_diff` 门禁、“可更新”批量处理 | 未做 | 代码在（`ReprocessDiffCard`）；没有用例 |
| 9 | `puppyone share` 与 `oo context mount` 产物一致 | 未做 | P11 对比的是一键执行与 CLI（L4）；`puppyone share` 薄封装属 puppyone-cloud，不在这里 |
| – | 模型配置与用量 | T1 通过 | 同 L4 1：`logs/model-extract.log` |

## 20 §7 的 T 卡

- F-S10-04：T1 drift/refresh T1 通过、media/queue-and-decode T1 通过（部分）、model T1 通过。
- F-S10-08：T2 P22–P25 T2 已写未跑。T3：未做。
- T4 owner 包：已构建（未签名），见 `../../desktop-p9/packs.md`；L6 清单在 L4 清单后追加 18–25 步。**应用启动在构建机上没能确认。**
