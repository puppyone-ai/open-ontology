# L4 CLI 验收包（owner）

阶段一（D80，30 §7.2）在 L4 结束时交付的验收清单：不用 Desktop，只用 `oo` 命令行，从启动引擎到撤销共享走完第一版的全部场景。每一步写“做什么”和“应该看到什么”；命令输出里的 id、端口、时间每次不同，只看**加粗**的部分。

每一步都由 `tools/oo-test/super_ontology_cli_acceptance.py` 在同一个 compose 运行时上逐条执行过（见末尾“已验证的运行”），脚本里的命令与本文一致。

约 20 分钟，其中第 4 步（首次解析 24 个文件，含 docx、pptx、pdf）约 1 到 3 分钟。

## 0. 准备

需要：Docker 已启动；仓库在 `feat/super-ontology` 的验收提交上；本机没有别的 `oo serve` 在跑。

```bash
cd /path/to/open-ontology            # 验收提交的检出目录
export CARGO_BUILD_JOBS=6
cargo build --locked -p oo-cli --features docs
export OO=$PWD/target/debug/oo
export STATE=$HOME/oo-acceptance/state
export DRIVE=$HOME/oo-acceptance/sample-drive
mkdir -p "$STATE" && chmod 700 "$STATE"
```

镜像（`oo-docs-engine:dev`、`oo-docs-parser:dev`）已在本机，摘要记在 `deploy/docs/images.lock.json`；`oo serve --runtime compose` 启动时会逐个核对，不符直接退出 4。

示例资料：评测集的子集（24 个文件：Decisions、People、Projects 的 md，三份 docx 会议纪要，两份 xlsx 表，一份 pdf 方案，一份 pptx 周报，一份 csv，一份 txt）加两份故意读不了的文件（加密 pdf、旧版 .doc）。

```bash
mkdir -p "$DRIVE"
G=testing/super-ontology/golden/docs
for d in Decisions People Projects; do mkdir -p "$DRIVE/$d"; cp $G/$d/*.md "$DRIVE/$d/"; done
mkdir -p "$DRIVE/Meetings" "$DRIVE/Tables" "$DRIVE/Proposals" "$DRIVE/Weekly" "$DRIVE/Data" "$DRIVE/Notes" "$DRIVE/Broken"
cp "$G/Meetings/2026-01-08-年度计划会.docx" "$G/Meetings/2026-02-15-星图技术评审.docx" "$G/Meetings/2026-06-01-项目分工调整.docx" "$DRIVE/Meetings/"
cp "$G/Tables/人员表.xlsx" "$G/Tables/项目负责人表.xlsx" "$DRIVE/Tables/"
cp "$G/Proposals/星图看板产品方案.pdf" "$DRIVE/Proposals/"
cp "$G/Weekly/2026-W22-周报.pptx" "$DRIVE/Weekly/"
cp "$G/Data/任务清单.csv" "$DRIVE/Data/"
cp "$G/Notes/速记-0412.txt" "$DRIVE/Notes/"
cp testing/super-ontology/fixtures/files/pdf-encrypted.pdf testing/super-ontology/fixtures/files/legacy.doc "$DRIVE/Broken/"
find "$DRIVE" -type f | wc -l
```

**应看到**：`26`。

## 1. 启动引擎（终端 A）

```bash
$OO serve --runtime compose --json --state-dir "$STATE"
```

这个终端保持打开。第一次启动约 1 到 2 分钟（拉起三个容器）。

**应看到**：一行 JSON，包含 `"protocol":"oo.docs.serve/v1"`、一个本地 `port`、`"engine":{"mode":"compose","project":"oo-docs-<12 位十六进制>", ...}`，`images` 里三个镜像摘要与 `deploy/docs/images.lock.json` 一致。守护进程只监听 `127.0.0.1`。

另开终端 B，设同样的 `OO`、`STATE` 环境变量，后面的命令都在终端 B 执行。

```bash
$OO serve --status --state-dir "$STATE"
```

**应看到**：`oo serve is running (pid <n>, port <p>)`。

## 2. 检查健康

```bash
$OO docs health --state-dir "$STATE"
```

**应看到**：一份 JSON，**`status` 为 `ok`**（刚启动时可能是 `degraded` 几秒），`components.db.status`、`cas`、`engine` 都是 `ok`，`components.parser.detail.kind` 是 `local+sidecar` 且 `formats` 含 `pdf`、`docx`、`pptx`，`components.model.status` 是 `not_configured`（`oo docs health` 目前不反映模型配置；第一版验收不配模型，配置后用 `GET /v1/docs/usage` 看 `model.status`，见第 15 步）。

## 3. 导入示例资料并观察状态

```bash
$OO docs drive add --name acceptance --root "$DRIVE" --state-dir "$STATE"
```

**应看到**：一行 `added <drive-id>  acceptance  active  files 0  syncing  rev 0`。

```bash
$OO docs files ls --drive acceptance --state-dir "$STATE"      # 每隔几秒执行一次
```

**应看到**：每行是 `<file-id>  <状态>  <路径>`。状态依次从 `queued`、`parsing`、`parsed`、`extracting`、`indexed` 走到 **`published`**；26 个文件约 1 到 3 分钟全部落定。最终：

* 24 个示例文件是 `published`（md、txt、csv、xlsx、docx、pptx、pdf 都有）；
* **`Broken/pdf-encrypted.pdf` 是 `encrypted`，`Broken/legacy.doc` 是 `unsupported`**（故意放的两个读不了的文件）；
* `Projects/官网改版.md` 可能在 `indexed` 停几秒，直到第 8 步处理它的冲突之前一直不会发布（见第 8 步），这是预期行为。

```bash
$OO docs drive show acceptance --state-dir "$STATE"
```

**应看到**：`<drive-id>  acceptance  active  files 26  <新鲜度>  rev <n>`，新鲜度先是 `processing`/`publishing`，之后因为有一个待确认冲突显示 `waiting_review`。

## 4. 看失败文件的原因

```bash
$OO docs files ls --drive acceptance --state encrypted --state-dir "$STATE"      # 取到 file-id
$OO docs files show <那个 file-id> --state-dir "$STATE"
```

**应看到**：`<file-id>  encrypted  Broken/pdf-encrypted.pdf  (DOCS_PARSE_ENCRYPTED)`；`legacy.doc` 对应 `unsupported ... (DOCS_PARSE_LEGACY_OFFICE)`。

```bash
$OO docs files retry <pdf 的 file-id> <doc 的 file-id> --state-dir "$STATE"
```

**应看到**：每个文件一行 `<file-id>  <结果>`；这两个文件是终态，结果不是 `queued`（它们不会因重试而变好）；对 `published` 的文件，结果是 `not_retryable`，原因 `state:published`。

## 5. 看一份会议纪要识别出的人和项目

```bash
$OO docs entity search 林远舟 --type Person --state-dir "$STATE"
$OO docs entity show <上一步的 key> --history --state-dir "$STATE"
```

**应看到**：搜索得到一行 `person:<24 位十六进制>  Person  林远舟`；`show` 先给出 `... (r1)`，然后每个别名一行（`alias 老林`、`alias Evan Lin`），再是事实列表，每行 `<fact-id> published  <关系> <对象> (r1)`，例如 `decides 周报每周五发送`、`member_of 管理`、`attr.role {"text":"CEO"}`。加 `--json` 可以看到每条事实的 `evidence`：`path`、`anchor`、`src`（例如 `[src:Meetings/2026-01-08-年度计划会.docx#¶3-3]`）。

## 6. 待确认队列

```bash
$OO docs review ls --status pending --state-dir "$STATE"
```

**应看到**：一行 `<item-id>  r1  constraint_conflict  pending  Projects/官网改版.md`：示例资料里真有一个冲突（同一个项目，两份文件写了两个负责人），引擎没有发布其中任何一个，等你选。

```bash
$OO docs review ls --kind constraint_conflict --json --state-dir "$STATE"     # 在 subject.fact_ids 里取到两个 fact-id
$OO docs review decide accept <item-id> --state-dir "$STATE"
```

**应看到**：第二条命令**不会接受**：`accepted 0, rejected 0`，并在 JSON 里给出 `DOCS_REVIEW_CONSTRAINT_CONFLICT`（接受冲突必须指明留下哪一个）。

```bash
$OO docs review decide accept <item-id> --winner <留下的 fact-id> --state-dir "$STATE"
```

**应看到**：`accepted 1, rejected 0; a new version is being published`。几秒后 `oo docs review ls --status pending` 显示 `the queue is empty`，`Projects/官网改版.md` 变为 `published`，`drive show` 的新鲜度不再是 `waiting_review`。

（`oo docs review accept-source <file-id>` 一次接受某个来源的全部低置信条目；示例资料里只有这一个冲突，所以它对 `官网改版.md` 返回 `accepted 0`，用于有低置信事实的资料，比如配置了模型之后。）

## 7. 实体纠错、拆分、合并

```bash
$OO docs entity search 老林 --state-dir "$STATE"                              # 别名解析到同一个人
$OO docs entity split <林远舟的 key> --groups '[{"aliases":["老林","林总"]},{"aliases":["Evan Lin"]}]' --state-dir "$STATE"
$OO docs entity merge --into <林远舟的 key> <split 返回的新 key> --state-dir "$STATE"
```

**应看到**：搜索“老林”得到 `person:...  Person  林远舟`（与上一步同一个 key）；`split` 输出 `split <key>: 1 new entities <新 key>`；`merge` 输出 `merged <新 key> into <key>`，之后新 key 在 `redirect_from` 里，原实体 `revision` 加一。

```bash
$OO docs entity correct <某条 mentions 事实的 fact-id> --mark-wrong --reason acceptance --entity <key> --state-dir "$STATE"
```

**应看到**：`<fact-id> -> retracted (origin rule)`；再 `entity show --history` 时这条事实的状态是 `retracted`，它的出处仍在。

## 8. 补一个新文件并同步

```bash
mkdir -p "$DRIVE/Notes" && printf '# 刚放进去\n\n负责人：孙晓棠\n' > "$DRIVE/Notes/刚放进去.md"
$OO docs drive sync acceptance --state-dir "$STATE"
$OO docs files ls --drive acceptance --state-dir "$STATE"
```

**应看到**：`sync running: generation <n>, +0 ~0 -0, drive_rev <m>`（同步在后台进行，立刻返回）；过几秒 `files ls` 里出现 `Notes/刚放进去.md`，状态走到 `published`。也可以只同步一个文件：`$OO docs drive sync acceptance --path Notes/刚放进去.md --state-dir "$STATE"`，输出 `sync complete: generation <n>, +0 ~1 -0, ...`。

## 9. 共享给 Agent（mount）

```bash
mkdir -p "$HOME/oo-acceptance/work"
$OO context mount --to "$HOME/oo-acceptance/work" --scope Decisions --scope Projects --for "星图看板用什么图表方案" --state-dir "$STATE"
$OO context list --state-dir "$STATE"
```

**应看到**：`mounted <目录>/.context (share <id>, BRIEF 5xx/800 tokens, trimmed 0, blocked <n>, expires <7 天后>)`；`list` 一行 `<share-id>  active  v<n>  expires ...  <目录>`。

## 10. 检查 BRIEF

```bash
head -22 "$HOME/oo-acceptance/work/.context/BRIEF.md"
```

**应看到**：第一行是“AI生成 · 由 Super Ontology 根据您的文件自动整理，可能有误，请以原文为准。”，接着是 AIGC 的 front matter；正文开头是 `> 本体版本 v<n>（<日期>）· 范围 Decisions、Projects · 预算 <x>/800 tokens · 角色 suggester`，下面一行 **`> 覆盖：11/11 个文件已处理`**，再下面是**一致性承诺**一行（包内一致、新鲜度下界、变更必达、并行同源、本体无矛盾）；然后是“你在做什么”“相关的人”“当前状态与决定”等小节，**每一行内容的末尾都有 `[src:路径#位置]`**；篇幅在预算之内。文件末尾“已拦截”一节只有数量，没有被拦下的任何名字。

## 11. P21：用过的决定被推翻

两个 Agent 做同一个任务，各挂一份包（同一任务组）：

```bash
mkdir -p "$HOME/oo-acceptance/agent-a" "$HOME/oo-acceptance/agent-b"
$OO context mount --to "$HOME/oo-acceptance/agent-a" --scope Decisions --scope Projects --group TICKET-P21 --for "星图看板用什么图表方案" --refresh-policy manual --state-dir "$STATE"
$OO context mount --to "$HOME/oo-acceptance/agent-b" --scope Decisions --scope People   --group TICKET-P21 --for "林远舟的决定" --state-dir "$STATE"
$OO context check --dir "$HOME/oo-acceptance/agent-a/.context" ; echo "exit $?"
```

**应看到**：`share <id>: up to date, continue`，`exit 0`（agent-b 同样）。

在实体里把 agent-a 用到的一个决定标成错误：

```bash
$OO docs entity search 开源图表库 --type Decision --state-dir "$STATE"
$OO docs entity show <decision 的 key> --state-dir "$STATE"
$OO docs entity correct <那条决定的 fact-id> --mark-wrong --reason "决定被推翻" --entity <decision 的 key> --state-dir "$STATE"
```

约 5 秒内：

```bash
tail -8 "$HOME/oo-acceptance/agent-a/.context/UPDATES.md"
$OO context check --dir "$HOME/oo-acceptance/agent-a/.context" ; echo "exit $?"
```

**应看到**：`UPDATES.md` 末尾多出 `## #1 · v<n> → v<n+1> · block · decisions.md#...`，写明“……已被撤回，没有替代”，其余文件内容不变；`check` 输出 `share <id>: BLOCK, a fact you used was withdrawn or conflicts; do not commit, refresh or ask a person`，**`exit 20`**。再对 agent-b 做 check：退出码 0、10 或 20，说明它自己的包用没用到那条决定。

组内一起刷新：

```bash
$OO context refresh --group TICKET-P21 --state-dir "$STATE"
$OO context check --dir "$HOME/oo-acceptance/agent-a/.context" ; echo "exit $?"
```

**应看到**：`refreshed 2 shares to v<n+1>`（两个成员同一个新版本，全成或全不成），旧的显示为 `refreshed`，两个目录里的 `.context` 是新包；`check` 回到 `exit 0`。示例文件夹里的任何文件都没有被改动（可用 `find "$DRIVE" -type f | xargs shasum` 前后对比）。

## 12. 撤销

```bash
$OO context revoke "$HOME/oo-acceptance/work" --state-dir "$STATE"
$OO docs share ls --state-dir "$STATE"
```

**应看到**：`revoked <share-id>; local mount removed. Content already read cannot be taken back.`（已读内容无法收回）；目录 `.context` 被删除；`share ls` 里这个共享是 `revoked`。再 `oo context revoke` 两个 agent 目录。

## 13. 重启后一切还在

在终端 A 按 Ctrl-C（或终端 B 执行 `$OO serve --stop --state-dir "$STATE"`），应在 90 秒内看到 `oo serve stopped`、守护进程退出码 0、三个容器停止但没有删除；再按第 1 步重新启动，然后：

```bash
$OO docs drive show acceptance --state-dir "$STATE"
$OO docs files ls --drive acceptance --state published --state-dir "$STATE"
$OO docs share ls --state-dir "$STATE"
```

**应看到**：Drive 还在，文件都还是 `published`，共享还在列表里；还没撤销的挂载目录没有被动过。

## 14. 删除 Drive 并停止

```bash
$OO docs drive rm acceptance --state-dir "$STATE"
$OO docs drive ls --state-dir "$STATE"
$OO serve --stop --state-dir "$STATE"
```

**应看到**：`deleting <drive-id>`；稍后 `drive ls` 为 `no drives`；`oo serve stopped`。

## 15.（可选）配置模型后重新抽取

模型不是第一版验收的必要条件。要试：把端点写进一个 0600 文件、把 key 写进另一个 0600 文件（路径随意，**不要把它们的内容贴进任何命令或文件**），然后用控制 API 配置（把 `<token>` 换成 `$STATE/serve.token` 里的值，端口见 `serve.json`）：

```bash
curl -s -X PUT http://127.0.0.1:<port>/v1/docs/config/model -H "Authorization: Bearer $(cat $STATE/serve.token)" -H 'If-Match: "r1"' \
  -d '{"provider":"openai-compatible","base_url":"file:/abs/path/endpoint","model":"<model-id>","key_ref":"file:/abs/path/key"}'
```

key 只接受 `file:/绝对路径` 或 `env:NAME`；直接写 key 的值会得到 400 而且不会回显。compose 运行时里，引擎容器自己起模型出口进程（镜像里带 `/opt/oo/model_egress.py`，容器在 `docs-egress` 网络上）：守护进程收到这条配置时，会把 `file:` 引用的两个文件以只读方式绑定到容器里的**同一路径**，重建一次引擎容器，再把配置交给它；`env:NAME` 不行（容器看不到宿主的环境变量）。配置成功后 `GET /v1/docs/usage` 的 `model.status` 是 `ok`。评测里的模型数字见 `eval/README.md`。

## 已验证的运行

`tools/oo-test/super_ontology_cli_acceptance.py` 把第 1 到 14 步在临时目录里逐条执行并检查了上面的“应看到”（`cli-acceptance-run.json` 是每条命令的退出码和输出，`cli-acceptance-run.txt` 是检查清单）；红队门禁见 `redteam-gate.txt`。已知的差别：脚本用 `--json` 取字段，人读输出与上面一致；第 15 步没有脚本化。
