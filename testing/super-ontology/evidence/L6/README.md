# L6 多模态与演进 · OO 侧签收证据（D80 阶段一）

**签收（协调者，2026-11-18）**：此前暂缓的验收 1 与验收 3 已由独立验证员重跑通过：
- `result-verify-longruns.json`（@869fe07f3）：44 个扫描件经引擎全部 `published`，核心类型召回 0.8556（≥ 0.65），ocr-parser 峰值 6854 MiB（上限 10 GiB）；2 小时会议核心类型召回 1.0（≥ 0.6），media-parser 峰值 1399 MiB。
- `result-verify-longruns-2.json`（@57f4d824e）：杀进程续跑复用 40 = 中断前完成 40、重算 218，重放逐字节一致，说话人特征目录已删除；`speaker_live` 2/2；图表 0.9412、截图 1.0 字符召回（≥ 0.9，ocr-parser sidecar 测量）；sidecar 扫描子集核心召回 0.8556（77/90）。
- 已知差异：执行者的 sidecar 子集召回为 0.9651（83/86），独立重跑为 0.8556，差在 Proposals PDF 的 OCR 几乎为空（与经引擎 pdf 路径缺 7 个方案标题是同一处）；本机缺 Songti 字体，渲染改用 STHeiti。仍高于门槛，原因记为 OCR 线的待查项。

签收版本：OO `feat/super-ontology` @ a54c40586。独立复核运行于 `7ec049f0b`（分支 `so/V46-signoff`，2026-11-18，验证员与执行者分离），结果 `result-verify-signoff.json`：状态 pass，L6 CLI 验收 36/36、L6 端到端 28 通过 0 失败。长跑数字（2 小时会议、44 个扫描件）来自执行者在 compose 上的运行（`be2-l6rem-final` 等），独立复核没有重跑这两个长任务。L6 CLI 验收包（30 §7.2 的 owner 验收包）见 `ACCEPTANCE-CLI.md`。macOS 宿主机解码、Desktop 内的 `#t=` 跳转播放、说话人确认界面等 Desktop 部分按 30 §7 在阶段二补齐。

## 独立复核里的发现（如实记录）

- L6 端到端脚本首次运行失败：`python3 -I` 下缺 `pillow`；用 `uv --with pillow` 重跑 28/28。文档中记载的 `/usr/bin/python3` 路径没有在该环境测试 PIL。
- CLI 验收第 8 项的标签写 p95，实际断言的是最慢文件 ≤ 7 s（实测 `People/林川.md` 2015 ms、`Projects/星火计划.md` 1735 ms）。V2 审计另指出：第 1 项“health”曾允许 `degraded` 通过（现要求 `ok`，复核实测 status ok），第 8 项的编辑到 BRIEF 阈值为 30 s 而不是 D49 的 7 s（口径是 BRIEF 更新而非单文件延迟）。
- 验证时 `evidence/L6/formal.md` 在 `7ec049f0b` 不存在，它在 `823db4116` 上；现已在树里，L6 在 D81 下没有新的形式化项。

## 30 §L6 验收（OO 侧口径，30 §7.2）

| # | 验收项 | 结果 | 证据 |
|---|---|---|---|
| 1 | 扫描件子集 OCR 后核心类型召回 ≥ 0.65；图表、截图文字字符召回 ≥ 0.9 | 通过：经引擎（compose）44 个文件（56 页图片，源文件均为纯图片 pdf）全部 `published`（1502 s），核心类型召回 0.8605（74/86，Decision 10/10、Person 9/9、Organization 4/4、Project 4/4、Term 5/5、Event 4/5、Topic 4/5、Document 34/44）；ocr-parser 峰值内存 6833 MiB（上限 10 GiB）。12 个缺失：三个速记标签（sidecar 也没认出）、七个方案 pdf 的标题（sidecar 对页面图片得 0.9651，但经引擎的 pdf 路径没有返回，**原因未调查**）、1 个主题和 1 个事件。图表与截图文字 0.9706 是 **sidecar 单独**的数字，不是引擎路径；一份扫描 pdf 与一张 png 把 3 个埋入的词带过整条栈（3/3） | `l6-long-ocr44.json`、`ocr-recall.md`、`l6-e2e-result.json` |
| 2 | ASR 与说话人分离比测报告已签收，选型与许可判定写入 `supply-chain/` | 通过：报告 `asr-bakeoff.md`（合成 3 人 17 轮 96 s 会议；Qwen3-ASR-0.6B CER 0.0674 但 float32 需 6 GB 超过 4 GiB 上限、bfloat16 慢 10 倍；Paraformer-zh CER 0.1348 需 torch；FireRedASR2 与 pyannote 未评测；`qwen-asr` 包因 GPL 依赖被禁；TTS 非开放 TTS，数据为合成，不含真实录音）。选型与许可判定已写入 `supply-chain/media-asr-model-reviews.json`（`b5384bfb5`）：CPU 默认为 Paraformer-zh 量化 ONNX + 3D-Speaker CAM++ ONNX + silero VAD，合成会议端到端 CER 0.174、DER 0.138；随镜像分发的三项（paraformer-zh、CAM++、silero-vad）许可均为默认可用，并列入 D67 律师门禁；记录的审阅者是工程协调者，**不是法律意见** | `asr-bakeoff.md`、`supply-chain/media-asr-model-reviews.json` |
| 3 | 2 小时合成 3 人会议：核心类型召回 ≥ 0.6；确认说话人后事实主语正确；中途杀 `media-parser` 续跑不重做已完成块；同一 ISA 指纹下逐字节可重放；说话人特征文件处理后不存在 | 通过（执行者运行）：把 golden 3 人会议重复 65 遍并埋入 12 段自我介绍，以 Opus/ogg 输入（2 小时 wav 为 230 MB，超过 200 MiB 文件上限，所以不是 wav），580 s `published`，核心类型召回 1.000（golden 会议的 6/6 个名字出现在转写里，样本只有 6 个名字），`media-parser` 峰值 1382 MiB，1064 个转写块；说话人确认前每个标签一条待确认项、没有事实挂在具名 Person 下，确认 S1 后事实主语正确；杀进程续跑复用 40 个块、重算 219 个；回放逐字节相同；说话人特征目录已不存在。**如实记录：12 段埋入的自我介绍 ASR 只听对 5 段、其中 2 段变成 `owns` 事实；ASR 文本没有标点，规则抽取很少匹配“我是X。我负责Y。”（给抽取线的发现）** | `l6-long-meeting.json`、`long-audio.json`、`l6-long-meeting-transcript.txt` |
| 4 | 合成录屏：每页幻灯片恰好一个关键帧；帧文字召回 ≥ 0.9；`#t=` 出处在 Desktop 能跳转播放 | OO 侧通过：2 页幻灯片、2 个 `frame_text` 块、字符召回 1.00、块带 `t_start_ms`；CLI 里 ref 为 `Meetings/录屏.webm#t=00:00:05-00:00:13`，会议 ref 为 `#t=00:00:00-00:00:05@S1`。样本只有 2 页。Desktop 内跳转播放为**阶段二** | `l6-e2e-result.json`、`cli-acceptance-l6-*` |
| 5 | macOS 上 H.264/AAC 的 mp4、m4a 经宿主机解码成功；容器内 FFmpeg `-buildconf` 不含 gpl、nonfree；无宿主机解码器且未配外部 FFmpeg 时显示 `DOCS_PARSE_CODEC_UNAVAILABLE` | 容器侧通过：mp4（H.264/AAC）与 m4a 为 `unsupported DOCS_PARSE_CODEC_UNAVAILABLE`，覆盖行 `2/4 个文件已处理；未完成 2 个` 列出两者；`-buildconf` 与无出网证明 `FFMPEG GATE: PASS`。**macOS 宿主机解码为阶段二（F-S10-05），未做** | `l6-docs-media.txt`、`cli-acceptance-l6-run.txt` |
| 6 | SBOM 许可门禁对 `ocr-parser`、`media-parser` 镜像全绿；`THIRD_PARTY_NOTICES/` 含全部新组件 | 门禁通过：两个镜像 `SBOM gate: PASS`（含 GCC 运行时例外与自编 LGPL FFmpeg 的审阅记录）。通知文件在 `tools/ocr-parser/`、`tools/media-parser/` 的 `THIRD_PARTY_NOTICES.md`，本次没有逐组件核对“全部新组件”是否齐全 | `sbom-ocr-parser.*`、`sbom-media-parser.*` |
| 7 | 修改被 Share 引用的决定，刷新后 BRIEF 头部列出差异 | 通过：刷新后的 BRIEF 有 `## 自上次以来的变化（v5 → v6）`；CLI 验收里改动 6 秒后 `check` 为 `BLOCK`（退出 20），`share refresh` 得到 version 7，BRIEF 列出 `进行中 → 已暂停`，`check` 回到 0 | `a7-BRIEF-after-refresh.md`、`cli-acceptance-l6-BRIEF-after-refresh.md` |
| 8 | 升级解析器版本：重跑期间已发布事实可用，完成后差异进门禁，无整库抖动 | 通过：引擎启动时 4 个文件 `outdated`，重跑期间旧事实保持发布（最少 15/15），事实键不变、0 条撤回，DOC-G10 持有 0 项，4 个文件（含会议 wav）都回到 `published`（e2e 28/28）。此前一次失败的原因是发布器把等人确认的草稿算作“还有未发布内容”，已修复（`speaker_live` 新增用例）。一次运行（`be2-l6rem-c`）里 DOC-G10 持有了确认说话人后的重跑，现不再卡住文件状态，但会向人询问 | `l6-e2e-result.json`、`l6-e2e-log.txt` |
| 9 | `puppyone share` 与 `oo context mount` 产物一致 | **阶段二，未做**（`puppyone share` 属 puppyone-cloud 的 CLI） | — |
| 10 | 媒体作业运行时，文档单文件延迟仍满足 D49 | 通过：独立复核 CLI 验收第 8 项两个文件检测到 published 为 2015 ms、1735 ms（≤ 7 s）；执行者运行：媒体作业运行时编辑 md 3.00 s，编辑到新 BRIEF 6.3 s（此前 40.9 s、20.2 s，根因是新鲜度等待取了整个 Drive 的最小覆盖版本，被一个长录音拖住；现在等待范围内文件结算，`coverage_scope_live`）；CLI 验收包里 5.7 s | `l6-e2e-result.json`、`a10-timeline.txt`、`result-verify-signoff.json` |

## 已知问题与发现

- 经引擎的 pdf 路径少返回 7 个方案标题（sidecar 对页面图片能识别）；原因未调查，属 OCR 线。
- 说话人确认后重跑可能让事实得到新键，DOC-G10 会因 `diff_over_30_percent` 持有它，文件状态不再受影响但需要人确认一次。
- `oo docs health` 不反映 ocr、media 两个 sidecar，也不列图片与音视频格式（已知）。
- 本机本地镜像被后续构建覆盖，CLI 验收用只改摘要的 `deploy/docs` 副本（`OO_ACCEPT_DEPLOY_DIR`）运行，仓库锁文件未改动，需集成线程重建后重新锁定。
- 阶段二：F-S10-05 macOS 宿主机解码 helper、F-S10-07 `#t=` 跳转、F-S10-06 说话人确认界面、关键帧条带与刷新预览、`puppyone share` 一致性。

## 历史记录（已被签收结论取代）

下面是签收前执行者的 L6 证据（`be2-l6rem-final`，28/28）。

# L6 evidence (30 section L6 acceptance, B-L6-2 close and B-L6rem)

Compose run `be2-l6rem-final` (engine rebuilt from the tree rebased on lane C's merge) of `tools/oo-test/super_ontology_l6.py --suite e2e` (`oo serve --runtime compose` with the `ocr` and
`media` profiles; engine image `oo-docs-engine:dev-be2`, media image `oo-docs-media-parser:dev-be2`, both in
`deploy/docs/images.lock.json`), plus the L1 cell evidence that already sat in this directory. `l6-e2e-result.json` and
`l6-e2e-log.txt` are the run; **28 of 28 checks pass** (the earlier run `be2-l6e2e-10` had 27 of 29: A8 and the A10 BRIEF bound).
Two long runs on the same stack, `tools/oo-test/super_ontology_l6_long.py`: the 2 hour meeting (`l6-long-meeting*.{json,txt}`) and
the 44 file scanned subset (`l6-long-ocr44*`). L2 stays green (`../L2`).

| # | Acceptance | Result | Numbers |
|---|---|---|---|
| 1 | scanned subset core recall >= 0.65 end to end; embedded chart text >= 0.9 | PASS | **through the engine (compose)**: the 44 files (56 page images, every source file as an image only pdf, `l6-long-ocr44.json`) all reach `published` in 1502 s, core type recall **0.8605** (74 of 86; Decision 10/10, Person 9/9, Organization 4/4, Project 4/4, Term 5/5, Event 4/5, Topic 4/5, Document 34/44), peak memory of the ocr-parser container 6833 MiB (limit 10 GiB). The 12 misses: the three note labels (`速记0412`, `速记0601`, `速记0828`, not text in a page; the sidecar missed them too), the titles of the seven proposal PDFs, one topic (客户满意度) and one event (客户沙龙). The sidecar alone, on the page images, scored 0.9651 (`ocr-recall.md`): through the pdf path of the engine the seven proposal titles were not returned, which is a finding for the OCR lane; the cause was not investigated here. Chart and screenshot text 0.9706 (sidecar), a scanned pdf and a png carry the 3 planted words through the stack (3/3).
| 2 | ASR and diarization bake-off | PASS (earlier card) | `asr-bakeoff.md` |
| 3 | 2 h meeting; speaker identity; kill and resume; ISA replay; no speaker features left | PASS | **2 h meeting through the real pipeline** (`l6-long-meeting.json`): the golden 3 person meeting repeated 65 times with 12 planted self introductions, written as Opus (15.4 MB; a 2 h wav is 230 MB, over the 200 MiB file limit); the recording reaches `published` in 580 s, **core type recall 1.000** (6 of 6 names the golden meeting says are in the engine's transcript; threshold 0.6), **peak memory 1382 MiB** (media-parser; docs-engine 38, docs-db 67, doc-parser 34), 1064 transcript blocks (`l6-long-meeting-transcript.txt`). Same as before: speaker identity (one item per label, nothing under a named Person before confirmation, after confirming S1 the fact's subject is that Person), kill and resume (reused 40, computed 219), byte identical replay, speaker feature directory gone (`long-audio.json`). Informational, not a threshold: of the 12 planted self introductions the ASR heard 5 with the project name right and 2 of those became `owns` facts; the ASR text has no punctuation and its mishearings of invented names (美晨项, 人海向) are the rest. Rules only, no model: the sentence rules rarely see `我是X。我负责Y。` in unpunctuated text, a finding for the extraction lane |
| 4 | video: one key frame per slide, frame text recall >= 0.9, `#t=` provenance | PASS | 2 slides, 2 `frame_text` blocks, character recall 1.00, blocks carry `t_start_ms`. Needed two sidecar fixes (FFmpeg 9 `-vsync`, scenedetect against an FFmpeg-free OpenCV), commit `Fix two media-parser defects` |
| 5 | Linux/compose: H.264/AAC is `DOCS_PARSE_CODEC_UNAVAILABLE`; `-buildconf` proof | PASS | mp4 (H.264/AAC) and m4a (AAC) are `unsupported DOCS_PARSE_CODEC_UNAVAILABLE`; `-buildconf` and the no egress proof in `l6-docs-media.txt` (`FFMPEG GATE: PASS`). macOS host decode is Desktop phase two |
| 7 | refresh diff in BRIEF head | PASS | after a changed fact the refreshed BRIEF has `## 自上次以来的变化（v5 → v6）` (`a7-BRIEF-after-refresh.md`) |
| 8 | parser version bump | PASS | the container version is written and found a real engine bug earlier (the engine read the sidecar version from a nested `parser` object). Now: 4 files `outdated` at engine start, old facts stay published during the rerun (min 15 of 15), no churn (same fact keys, 0 retracted), DOC-G10 held 0 items, and **all four files, the meeting wav included, return to `published`** (28 of 28). Cause of the earlier failure: the publisher only promotes a file whose facts it published in that round; the second path promotes every `indexed` file with nothing left to publish, and counted a draft that waits for a person as something left. Two kinds of draft wait like that and no longer hold the file: a statement of a speaker nobody has confirmed (`speaker_identity` item) and a fact of a new extraction that DOC-G10 holds (`reprocess_diff` item; one run, `be2-l6rem-c`, hit this: the confirmed speaker's fact got another key on the rerun, 1 of 2 keys changed, `diff_over_30_percent`, and the wav stayed `indexed`). Tests: `speaker_live::a_reprocessed_recording_keeps_its_confirmed_speaker_and_returns_to_published`. A confirmed label's fresh placeholder is merged into its Person right after the rerun (`apply_confirmed`) |
| 10 | media jobs running while document latency meets D49 | PASS | D49 as calibrated (md detected to published <= 7 s): **3.00 s** with a media job running at the edit. Edit to a new BRIEF: **6.3 s** (L2 without a media job: 5 to 6 s; the bound is 30 s), where the first run took 40.9 s and the second 20.2 s. `a10-timeline.txt` shows the document pipeline was never held back (see the timeline). The rest was the freshness wait of the compile: `share::coverage::prepare` waits up to 20 s (`wait_ms`) until the Drive's covered revision reaches the Drive's revision, and that number is the minimum over *every* file of the Drive, so one long recording still being parsed in `Meetings/` held every pack of `Projects/` for the full 20 s. The wait now ends as soon as the files of the pack's scope are settled; the number reported stays the Drive's (FM10 conformance still passes), the whole Drive still waits for everything. Test `coverage_scope_live`. (The first run's 30 s on top was the default fs poll, the script now sets 5 s as L2 does.) |
| 6, 9 | SBOM gate; `puppyone share` equality | 6 PASS (`sbom-*.json`); 9 phase two | |

Phase two (not here): F-S10-05 macOS host decode helper, F-S10-07 `#t=` jump in Desktop, F-S10-06 speaker confirm UI, the
Puppyone CLI `puppyone share` equality (acceptance 9).
