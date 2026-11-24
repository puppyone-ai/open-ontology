# L6 CLI 验收包（owner）

阶段一（D80，30 §7.2）在 L6 结束时交付的第二份验收清单：接在 [L4 的清单](../L4/ACCEPTANCE-CLI.md) 之后，只用 `oo` 命令行，验收 L6 新增的东西：扫描件与图片的文字（OCR）、会议录音与说话人确认、屏幕录屏、读不了的音视频、Share 的漂移与差异摘要、媒体作业运行时文档的单文件延迟。每一步写“做什么”和“应该看到什么”；id、端口、时间每次不同，只看**加粗**的部分。

每一步都由 `tools/oo-test/super_ontology_cli_acceptance_l6.py` 在同一个 compose 运行时（带 `ocr`、`media` 两个 sidecar）上逐条执行过，脚本里的命令与本文一致（见末尾“本树上的实测”）。示例资料全部是合成的，由 `tools/oo-test/super_ontology_l6_samples.py` 生成。

约 15 分钟：第 2 步解析 8 个文件（含一段会议录音和一段录屏）约 1 分钟，其余几乎是即时的。

**不在命令行包里的验收项**（它们不是 `oo` 命令能完成的，各有自己的脚本和证据）：升级解析器版本后的重跑（L6 验收 8，`super_ontology_l6_e2e.py` 的 A8）；2 小时会议与中途杀掉 `media-parser` 续跑（验收 3，`super_ontology_l6_long.py`）；OCR 与 ASR 的召回数字（验收 1、2，`ocr-recall.md`、`asr-bakeoff.md`）；SBOM 门禁（验收 6，`sbom-*.gate.txt`）；`puppyone share`（验收 9）和 macOS 宿主机解码（验收 5 的成功分支）属于阶段二。

## 0. 准备

需要：Docker 已启动；仓库在验收提交上；本机没有别的 `oo serve` 在跑；macOS（生成示例用到 `say`、`sips`）、系统 python（带 Pillow）、支持 libopus、libvpx-vp9、libx264 的 `ffmpeg`。这些宿主机工具只用来**做输入**，容器里解码用自己的 FFmpeg。

```bash
cd /path/to/open-ontology            # 验收提交的检出目录
export CARGO_BUILD_JOBS=6
cargo build --locked -p oo-cli --features docs
export OO=$PWD/target/debug/oo
export STATE=$HOME/oo-acceptance6/state
export DRIVE=$HOME/oo-acceptance6/sample-drive
mkdir -p "$STATE/engine" && chmod 700 "$STATE" "$STATE/engine"
printf '{"version":"oo.docs/v1","sync":{"fs_poll_seconds":5}}' > "$STATE/engine/docs.json"
/usr/bin/python3 tools/oo-test/super_ontology_l6_samples.py "$DRIVE"
```

第二条 printf 把文件系统轮询改成 5 秒（默认 30 秒），第 8 步的 30 秒上限是从引擎看到改动开始算的。

**应看到**：列出 8 个文件，最后一行 `8 files`：`Meetings/周会 参会=小王、小李.wav`（3 人 4 句话）、`Meetings/录屏.webm`（2 页幻灯片，VP9/Opus）、`Meetings/录屏h264.mp4`（H.264/AAC）、`Meetings/讲话.m4a`（AAC）、`Scans/扫描件.pdf`（只有图像、没有文字层）、`Scans/纪要.png`、`Projects/星火计划.md`、`People/林川.md`。

镜像：`oo-docs-engine`、`oo-docs-parser`、`oo-docs-ocr-parser`、`oo-docs-media-parser` 要在本机，摘要记在 `deploy/docs/images.lock.json`，`oo serve --runtime compose` 启动时逐个核对，不符退出 4。

## 1. 启动引擎，带上 ocr 和 media（终端 A）

```bash
export OO_DOCS_PROFILES=ocr,media
export OO_DOCS_MEDIA_IMAGE=$(python3 -c "import json;print(json.load(open('deploy/docs/images.lock.json'))['media_parser']['tag'])")
export OO_DOCS_OCR_IMAGE=$(python3 -c "import json;print(json.load(open('deploy/docs/images.lock.json'))['ocr_parser']['tag'])")
$OO serve --runtime compose --json --state-dir "$STATE"
```

这个终端保持打开；另开终端 B，设同样的 `OO`、`STATE`，后面的命令都在终端 B 执行。

**应看到**：一行 JSON，`"engine":{"mode":"compose", ...}` 和一个本地 `port`。

```bash
docker ps --format '{{.Names}}'
$OO docs health --state-dir "$STATE"
```

**应看到**：容器里除了 `docs-engine`、`docs-db`、`doc-parser`，还有 **`ocr-parser` 和 `media-parser`**；`health` 的 **`status` 为 `ok`**。（已知：`health` 的 `parser.detail.formats` 目前只列文档类格式，不列图片和音视频；两个 sidecar 以容器为准。）

## 2. 登记示例 Drive，观察状态

```bash
$OO docs drive add --name l6 --root "$DRIVE" --state-dir "$STATE"
$OO docs files ls --drive l6 --state-dir "$STATE"      # 每隔几秒执行一次
```

**应看到**：约 1 分钟内 8 个文件全部落定：**6 个 `published`**（两个 md、`扫描件.pdf`、`纪要.png`、会议 wav、录屏 webm），**2 个 `unsupported`**（`录屏h264.mp4`、`讲话.m4a`，见第 6 步）。音视频文件会先在 `parsing` 停一会儿。

## 3. OCR：扫描件的文字带着出处进入 Pack

```bash
mkdir -p "$HOME/oo-acceptance6/scan"
$OO context mount --to "$HOME/oo-acceptance6/scan" --scope Scans --for 纪要 --state-dir "$STATE"
ls "$HOME/oo-acceptance6/scan/.context/refs"
cat "$HOME/oo-acceptance6/scan/.context/refs/0002.md"
grep -n "Scans/" "$HOME/oo-acceptance6/scan/.context/BRIEF.md"
```

**应看到**：`mounted ... (share <id>, BRIEF 4xx/800 tokens, ...)`；`refs/` 里有几份“出处”文件，**其中 `文件：Scans/扫描件.pdf`、`位置：#p1`（以及 `文件：Scans/纪要.png`、`位置：#img`）的代码块里是 OCR 出来的三行原文：`2026年度计划会纪要`、`负责人：林川`、`项目：星火计划`**（同一页可能被引成几份 ref，每份引一段）；BRIEF 里至少一行的出处是 `[src:Scans/…#p1]` 或 `#img`。扫描 pdf 没有文字层，这些字只能来自 OCR。

## 4. 说话人确认

```bash
$OO docs review ls --kind speaker_identity --status pending --state-dir "$STATE"
$OO docs entity search 小王 --type Person --state-dir "$STATE"
```

**应看到**：**每个说话人标签一条 `speaker_identity` 待确认项**（会议 3 个标签 S1、S2、S3，录屏的旁白 1 个，共 4 条）；此时**搜不到叫“小王”的 Person**：没人确认之前，没有任何一条事实挂在某个真人名下。

```bash
$OO docs review ls --kind speaker_identity --json --state-dir "$STATE"      # 取会议文件里 subject.speaker 为 S1 的那条的 id
$OO docs review decide accept <S1 的 item-id> --name 小王 --state-dir "$STATE"
```

**应看到**：`accepted 1, rejected 0; a new version is being published`。（`--name` 是这次新加的：说话人确认要说出这个人是谁。）几秒到几十秒后：

```bash
$OO docs entity search 小王 --type Person --state-dir "$STATE"
$OO docs entity show <上一步的 key> --state-dir "$STATE"
$OO docs review ls --kind speaker_identity --status pending --state-dir "$STATE"
```

**应看到**：搜到 `person:<24 位十六进制>  Person  小王`；`show` 里有一个别名 `说话人 S1 · <8 位>`，事实里有 **`owns 星火计划…`（S1 自我介绍里说“我负责星火计划”，主语现在是 Person 小王）**；待确认队列里**还剩 3 条**（其余标签没人确认，就不发布）。

## 5. 屏幕录屏

```bash
$OO docs files show <录屏.webm 的 file-id> --state-dir "$STATE"
mkdir -p "$HOME/oo-acceptance6/meet"
$OO context mount --to "$HOME/oo-acceptance6/meet" --scope Meetings --for 星火计划九月上线 --state-dir "$STATE"
grep -l "录屏.webm" "$HOME/oo-acceptance6/meet/.context/refs/"*.md
cat "$HOME/oo-acceptance6/meet/.context/refs/0002.md"
grep -rn "#t=" "$HOME/oo-acceptance6/meet/.context/BRIEF.md" "$HOME/oo-acceptance6/meet/.context/projects/"
```

**应看到**：`files show` 的状态是 `published`；`refs/` 里有 `文件：Meetings/录屏.webm`、**`位置：#t=00:00:05-00:00:13`**，代码块里是幻灯片上识别出的字（`下季计划`、`星火计划九月上线`）；会议 wav 的 ref 是 **`位置：#t=00:00:00-00:00:05@S1`**（时间范围加说话人标签）；BRIEF 和 `projects/` 里引用它们的行以 `[src:Meetings/录屏.webm#t=…]`、`[src:Meetings/周会 参会=小王、小李.wav#t=…@S1]` 结尾。（第 1 页幻灯片的字只在被某个对象引用时才进 ref；“每页恰好一个关键帧、帧文字召回 ≥ 0.9”由 `super_ontology_l6_e2e.py` 的 A4 用控制 API 检查，命令行看不到关键帧本身。）

## 6. 没有宿主机解码器时，H.264 和 AAC 不能处理

```bash
$OO docs files show <录屏h264.mp4 的 file-id> --state-dir "$STATE"
$OO docs files show <讲话.m4a 的 file-id> --state-dir "$STATE"
sed -n '/覆盖/p' "$HOME/oo-acceptance6/meet/.context/BRIEF.md"
```

**应看到**：两个文件都是 **`unsupported`、`DOCS_PARSE_CODEC_UNAVAILABLE`**；BRIEF 的覆盖行如实写：**`> 覆盖：2/4 个文件已处理；未完成 2 个（Meetings/录屏h264.mp4 失败（DOCS_PARSE_CODEC_UNAVAILABLE）、Meetings/讲话.m4a 失败（DOCS_PARSE_CODEC_UNAVAILABLE））`**。（macOS 宿主机解码是阶段二的 Desktop helper，本包不覆盖。）

## 7. 漂移：Share 用过的事实变了

```bash
mkdir -p "$HOME/oo-acceptance6/p1"
$OO context mount --to "$HOME/oo-acceptance6/p1" --scope Projects --for 星火计划 --state-dir "$STATE"
sed -i '' 's/进行中/已暂停/' "$DRIVE/Projects/星火计划.md"      # Linux 用 sed -i
sleep 8
$OO context check --dir "$HOME/oo-acceptance6/p1/.context" ; echo "exit $?"
```

**应看到**：约 6 到 10 秒后 `check` 输出 `share <id>: BLOCK, a fact you used was withdrawn or conflicts; do not commit, refresh or ask a person`，**`exit 20`**（改动之前是 `up to date, continue`，`exit 0`）。

```bash
$OO docs share refresh <share-id> --state-dir "$STATE"
sed -n 1,30p "$HOME/oo-acceptance6/p1/.context/BRIEF.md"
$OO context check --dir "$HOME/oo-acceptance6/p1/.context" ; echo "exit $?"
```

**应看到**：`refreshed <旧 id> -> <新 id> (v<n>), mounted at <目录>/.context`；BRIEF 在头部之后多一节 **`## 自上次以来的变化（v<n-1> → v<n>）`**，里面一行 **`改值：星火计划：状态为 进行中 → 星火计划：状态为 已暂停 [src:Projects/星火计划.md#L10-L10]`**；`check` 回到 `exit 0`。

## 8. 媒体作业运行时，改一个 Markdown

做一段更长的录音（把会议 wav 循环 41 遍，不重新编码），让媒体作业忙起来，然后改 md，量它多久进入新的 BRIEF：

```bash
ffmpeg -nostdin -v error -y -stream_loop 40 -i "$DRIVE/Meetings/周会 参会=小王、小李.wav" -c copy "$DRIVE/Meetings/周会2 参会=小王、小李.wav"
$OO docs drive sync l6 --state-dir "$STATE"
$OO docs files ls --drive l6 --state-dir "$STATE" | grep 周会2      # 看到 parsing 等状态，说明媒体作业在跑
sed -i '' 's/已暂停/收尾中/' "$DRIVE/Projects/星火计划.md"
mkdir -p "$HOME/oo-acceptance6/m3"
time $OO context mount --to "$HOME/oo-acceptance6/m3" --scope Projects --for 星火计划 --state-dir "$STATE"
grep -c 收尾中 "$HOME/oo-acceptance6/m3/.context/BRIEF.md"
```

**应看到**：`周会2` 在 `queued`/`parsing`/`extracting` 之一；改动后的**第一次或第二次** mount 的 BRIEF 里就有 `收尾中`（`grep -c` 大于 0），改动到新 BRIEF 的时间**不超过 30 秒**（D49 与 L2 的上限）；如果第一次 mount 还是旧内容，`oo context revoke` 掉它，几秒后再 mount 一次。

## 9. 撤销并停止

```bash
for d in scan meet p1 m3; do $OO context revoke "$HOME/oo-acceptance6/$d" --state-dir "$STATE"; done
$OO docs share ls --state-dir "$STATE"
$OO docs drive rm l6 --state-dir "$STATE"
$OO serve --stop --state-dir "$STATE"
```

**应看到**：每个目录 `revoked <share-id>; local mount removed. Content already read cannot be taken back.`；`share ls` 里没有 `active` 的共享；`oo serve stopped`，守护进程退出码 0。

## 本树上的实测

运行编号 `a7-l6cli`，2026-11-18，分支 `so/A-L7-remainder`，`oo` 由该树构建，`--runtime compose`，`OO_DOCS_PROFILES=ocr,media`：**35 项检查全部通过**（`cli-acceptance-l6-run.json` 是每条命令的退出码和输出，`cli-acceptance-l6-run.txt` 是检查清单；`cli-acceptance-l6-*.txt|md|json` 是每一步取下来的原文）。第一次运行有 3 项没过：OCR 的检查把同一页的几份 ref 当成一份（脚本问题）、说话人确认后立刻读事实读到空（事实几十秒后才发布，脚本改成等待）、`share refresh` 返回的 JSON 字段我读错了；三处都是检查本身，已改，改后重跑全部通过。

| 步骤 | 实测 |
|---|---|
| 0 示例 | 8 个合成文件 |
| 1 启动 | 5 个容器（含 `ocr-parser`、`media-parser`），`health` `ok`；`health` 不列图片、音视频格式（已知） |
| 2 状态 | 8 个文件 **50 秒**内落定：6 `published`、2 `unsupported` |
| 3 OCR | Scans 的 Pack 有 4 份 ref：`扫描件.pdf#p1`（两份，一份引后两行、一份引整页）、`纪要.png#img`（整页，三行原文都在）；BRIEF 有 `[src:Scans/…]` 出处 |
| 4 说话人 | 4 条 `speaker_identity`（会议 S1、S2、S3，录屏旁白 1）；确认前搜“小王”0 条；`decide accept --name 小王` 为 `accepted 1`；之后 Person 小王有 `owns` 事实（`published`），队列还剩 3 条 |
| 5 录屏 | `files show` `published`；ref `Meetings/录屏.webm` 位置 `#t=00:00:05-00:00:13`，字是 `下季计划 / 星火计划九月上线`；会议 ref `#t=00:00:00-00:00:05@S1` |
| 6 读不了的 | `录屏h264.mp4`、`讲话.m4a` 都是 `unsupported DOCS_PARSE_CODEC_UNAVAILABLE`；BRIEF 覆盖行 `2/4 个文件已处理；未完成 2 个` 列出两者 |
| 7 漂移 | 改动后 6 秒 `check` 为 `BLOCK`，`exit 20`；`share refresh` 返回 `version 7`；BRIEF `## 自上次以来的变化` 里有 `进行中 → 已暂停`；`check` 回到 `exit 0` |
| 8 延迟 | 长录音在 `parsing` 时改 md，改动到新 BRIEF **5.7 秒**（上限 30 秒；中间有 3 次读到旧内容的 mount，已撤销） |
| 9 收尾 | 所有 Share 为 `revoked`（刷新过的一份为 `refreshed`）；`oo serve stopped`，退出码 0；没有残留容器、网络、卷 |

**镜像说明**：这台机器上的本地镜像（`oo-docs-engine:dev`、`oo-docs-parser:dev`、`oo-docs-ocr-parser:dev`）已被后来的构建覆盖，摘要和 `deploy/docs/images.lock.json` 里记的不同，`oo serve` 因此按设计退出 4（`digest mismatch`）。本次运行用一份只改了镜像摘要的 `deploy/docs` 副本（脚本的环境变量 `OO_ACCEPT_DEPLOY_DIR`，同时给 `oo serve --deploy-dir`），其余文件与仓库一致；仓库里的锁文件没有改动，由集成线程在重建镜像后重新锁定。镜像里的引擎二进制因此不一定带有本分支的改动；本包的步骤不依赖它们。

**已知的差别与缺口**：脚本用 `--json` 取字段，人读输出与上面一致；第 5 步看不到“每页一个关键帧”本身；`oo docs health` 不反映 ocr、media 两个 sidecar；说话人确认之前 `docs entity show` 里的 Person 还没有事实，要等新版本发布。
