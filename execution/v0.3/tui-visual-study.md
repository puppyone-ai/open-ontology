# v0.3 TUI 视觉研究：ascii.rest 241 个素材

Plan: V3-A-02 · 分支 `v0.3/tui` · 2026-11-09

## 0. 研究方法

- 素材源：`bas3line/ascii` 仓库 `main`（2026-11-08 推送），`src/pieces/*.ts` 共 241 个素材 + `src/kit/*`
  （约 3 万行的公共工具：`fx.ts`、`glyphs.ts`、`materials.ts`、`field.ts`）。npm 包 `ascii.rest@0.4.0`
  只带 218 个，缺 `chat`、`git-graph`、`pipeline`、`edge-orbit` 等 23 个后加的，所以以 GitHub 源码为准。
- 源码下载到仓库外的 `/tmp/ascii-rest-src/`，渲染脚本放在另一个目录，逐个 `import` 后在 t=2.5s 取一帧，
  241 个全部渲染一遍（共 244 段输出，个别素材多取了时间点）；`ui`、`data`、`type`、`effects`、`generative`
  五类逐帧细看，其余类别看元数据（尺寸、fps、调色板、note）加一帧样张。
- 分类统计（GitHub 源码）：scenes 34 · logos 29 · distros 23 · companies 23 · nature 16 · ui 15 ·
  physics 14 · objects 14 · generative 13 · shapes 12 · space 11 · creatures 10 · data 10 · type 9 · effects 8。
- 所有素材 MIT 许可（© 2026 bas3line）。移植到 `oo` 的代码见 §4，许可全文在
  `crates/oo-cli/src/tui/THIRD_PARTY.md`。

## 1. 共性手法（跨类别）

| 手法 | 在素材里的做法 | 对 `oo` 的取舍 |
|---|---|---|
| 字符明暗阶梯 | 最常见的是 `" .:-=+*#%@"`（15 个素材）、`" ·•●"`（34 个）、`" ░▒▓█"`、四分块 `▘▝▀▖▌▞▛▗▚▐▜▄▙▟█`、八分块 `▁▂▃▄▅▆▇█` / `▏▎▍▌▋▊▉` | 只用三套：点阶 `·•●`（装饰）、块阶 `░▒▓█`（热力、揭示）、八分块（进度、sparkline）。ASCII 符号阶在 CJK 字体里宽度不稳，不用 |
| 抖动 | 交错行偏移半个阈值（plasma、julia-set），或 hash 稀疏淡出（starfield、logo 边缘） | 保留现有 `fx::ramp` 的交错抖动；hash 稀疏只用于开场 |
| 色彩分层 | 109 个素材带 `palette`；kit 的 glint 把单元格本色往白色混 0.6（核心）/0.3（边缘），而不是换色相 | 新增两个色阶：`Ember`（暗橙，阴影/底层）与 `Hot`（亮白橙，高光）。整个 UI 只有一条暖色轴：Faint → Ember → Accent → Hot；状态色（绿/黄/红/蓝）只表达含义 |
| 单墨模式 | kit 的每个效果在 `mono` 时只改字符不改颜色（glint 在单色里把 `█` 换成 `▓`） | 同样的原则：单色下高亮靠字形（粗框 `┏━┓`、`▓`、反显），颜色只是第二层 |
| 动画节奏 | 一次性效果都有 `once`/`still`：例如 typeIn 打完停 0.5s；glint 默认每 4s 扫一次，扫一次 1.2s；scan 揭示占周期的 60% | 所有一次性动画 ≤ 1.2s，结束后停在“静止帧”，静止帧就是 `--no-anim` / `NO_COLOR` / 快照里看到的那一帧 |
| 缓动 | gauge 指针过冲；bar-chart 在数据集之间 ease；dissolve 每个字母错开延迟 | 用 `ease_out_cubic` + 按位置错开（stagger），不用弹簧过冲（终端 10fps 下过冲像抖动） |
| 文字揭示 | scramble（左→右从乱码落定，字符表 `!<>-_\/[]{}=+*^?#%&$@0-9a-f`）、typewriter（光标 `▌`）、dissolve（尘埃 `.·░▒▓█` 聚到笔画上）、split-flap（翻牌）、wave-text | 开场用 dissolve 聚合 + glint；标语与步骤名用 scramble；交付屏横幅用 glint |
| 确定性 | 每一帧是 `t` 的纯函数，`hash(seed, i)` 代替随机数，所以任何一帧都能先画 | 与 `oo` 现有约定一致：`fx` 与 `graph` 只吃帧号，不读时钟 |

## 2. 分类笔记

- **ui（15）**：最直接可用。`pipeline` 的“图标 · 名称 · 条 · 耗时 · 状态词 + 底部当前命令”是构建屏的现成版式；
  `progress-bar` 的五种条里 `▕████▍▏` 带端帽最清楚；`skeleton` 的斜向 `░▒▓▒░` 光带说明“正在加载”而不需要数字；
  `form-controls` 的 `[x]`/`(*)`、`box-frames` 的 12 种框、`dividers` 的渐隐分隔线可直接对照现有 theme；
  `spinners` 里 braille 点阵最安静；`git-graph`/`file-tree` 的线字形和我们图谱的正交走线同一套。
  `boot-log`、`terminal`、`chat` 说明“滚动日志 + 右侧状态词”比进度百分比更有“在干活”的感觉。
- **data（10）**：`sparkline`（▁…█ + 右侧当前值和 min/max）、`uptime-bar`（一格一天的 `▌` 条）、
  `heatmap`（`·░▒▓█` 五级 + 图例 `less · ░ ▒ ▓ █ more`）、`gauge`（半圆仪表）、`radar`（扫描线加余辉）、
  `cpu-meters`（htop 式 `[|||###  79%]`）、`equalizer`（峰值帽慢落）。共同点：数值永远印在图旁边，图只是加速判断。
- **type（9）**：`big-text` 用 5 行像素字体，再由“像素四邻”算出 `╗║═╝` 阴影，是终端里最有分量的标题；
  `dissolve`、`scramble`、`typewriter`、`split-flap` 是四种揭示；`glitch` 太吵，不用。
- **effects（8）**：`doom-fire`、`tunnel`、`rotozoomer`、`tv-static` 都是全屏刷新型，和“信息密度优先”冲突；
  `fireworks`/`sparks` 的短促粒子（`.`、`|`、`o` 加衰减）适合作一次性庆祝，前提是只出现在空白区且 1 秒内消失。
- **generative（13）**：`plasma`、`julia-set`、`mandelbrot` 证明 `.,-~:;=+*#%@` 阶梯在 10 行内能出体积感，但对我们是纯装饰；
  `hilbert-curve` 的“两个亮点沿路径跑”可借给图谱边的高亮；`maze` 的“先挖再解”对应图谱“先揭示再高亮”。
- **logos / companies / distros（75）**：几乎全是同一个模板——静态 logo + 每隔几秒一次的 `glint` 或 `scan`。
  说明一个标志只要“偶尔被光扫过”就足够有生命，不需要持续动画。开场的 wordmark 照此处理，但只扫一次。
- **shapes / space（23）**：`donut`、`cube`、`earth` 的 Lambert 光照 + 明暗阶梯；`starfield` 的中心外放与边缘拉线。
  旧开场的“点阵球”就是这一路，信息量为零，这次换掉。
- **physics / nature / creatures / objects / scenes（88）**：大都是 60×20 以上、带调色板的场景画。可借的是
  分层：远景用最暗的色、近景用亮色、只有一个焦点用高亮色（`lighthouse` 的光束、`lantern-lake` 的灯）。
  这正是我们“只有一个 Accent，选中才 Hot”的原则。`radar`/`lighthouse` 的“扫过即亮、之后余辉渐暗”被用在图谱揭示。

## 3. 选用的 18 个素材与落点

| # | 素材 | 用在哪 | ratatui 实现方式 |
|---|---|---|---|
| 1 | `big-text` | 开场 wordmark `OPEN ONTOLOGY` | 移植 5 行像素字体（用到的字母）与阴影连接表 `JOIN`；每像素 1 列。80 列放得下（76 列），更窄时退回间隔小写字。纯函数 `fx::wordmark()` 输出 `(char, Tone)` 网格 |
| 2 | `dissolve` | 开场：尘埃聚成 wordmark | 每个墨水格有按 x 排序加 hash 抖动的延迟，局部进度经 `ease_out_cubic` 后映射到 `·░▒▓█`，并从下方 1–3 行“升”到位 |
| 3 | `glint`（kit） | 开场 wordmark 收尾、交付屏 ready 横幅 | 斜向光带 `x + slant*y`，核心 `█`→Hot、边缘 `▓`；单色下只换字形 |
| 4 | `scramble` | 开场标语、步骤名 | 改写现有 `fx::scramble`：噪声字符表换成素材的 `!<>-_\/[]{}=+*^?#%&$@`，加“刚落定的字符先亮一帧”（Hot） |
| 5 | `typewriter` | 开场标语的光标 `▌` | 只在动画中出现，静止帧无光标 |
| 6 | `pipeline` | 构建屏阶段行 + 底部“当前” | 每行：节点 · 名称 · 条 · 计数 · 耗时 · 状态词（passed / running / queued / failed，中文 完成 / 进行中 / 排队 / 失败）；底部一行 `› 当前阶段的说明` |
| 7 | `progress-bar` | 构建条、选表计量条 | 八分块条加端帽 `▕…▏`，轨道用 `·` |
| 8 | `skeleton` | 构建屏正在跑的那条 | 已完成部分上有一条 3 格宽的 `▓` 光带按帧移动；冻结时不画 |
| 9 | `sparkline` | 构建屏吞吐量 | `▁▂▃▄▅▆▇█` 映射每秒完成单元数，右侧印最新值；历史由 app 每秒采样一次，快照用固定历史 |
| 10 | `heatmap` | 构建屏总进度热力条、选表行数 | 总进度条按阶段分段，每格按完成度取 `░▒▓█`；选表每行加 5 格对数行数条 `▁▃▅▇` |
| 11 | `uptime-bar` | 构建屏单阶段单元格 | 单元数 ≤ 条宽时一格一张表（`█` 完成 / `▓` 当前 / `·` 待做），否则按比例 |
| 12 | `gauge` | 构建屏总百分比 | 不画半圆（太占行），改成标题行右侧的大号百分比 + 分段仪表 |
| 13 | `radar` | 图谱确认屏揭示 | 以 hub 中心为原点，按“距离/最远距离”逐帧揭示；前沿 2 格画成 `·`/`░` 余辉，之后落定为真实字符 |
| 14 | `scan`（kit） | 连接屏检查项逐行出现 | 每项延迟 1 帧（100ms）出现，未出现的行留白 |
| 15 | `hilbert-curve` / `git-graph` | 图谱选中高亮 | 选中实体粗框（已有）+ 与它相连的边与基数标记变成 Accent，相邻实体框从 Faint 提到 Ink |
| 16 | `fireworks` / `sparks` | 交付屏 ready 一刻 | 1 秒内的一次绽放：从 ✓ 向外的 `· • ✦ +` 粒子，只落在空白格，结束后完全消失 |
| 17 | `box-frames` / `dividers` | 全局框架、卡片 | 主框圆角、选中粗框、卡片虚线 `┄┆`；标题纹理保留渐隐点线，其余普通屏不再有装饰点 |
| 18 | `cpu-meters` / `bar-chart` | 确认屏证据迷你图 | 决策行的置信度改成 5 格条 `▰▰▰▱▱ 0.91`；containment 证据画成带阈值刻度的条 `▕████▊·┊·▏0.71<0.98` |

没有选的：`glitch`、`tv-static`、`matrix-rain`、`doom-fire`、`tunnel`（满屏噪声，会干扰判断）；
`marquee` 的跑马灯（持续闪烁，违背“动画短、可跳过”）；所有场景画（尺寸 60×20 以上，没有信息量）。

## 4. 移植声明

- `crates/oo-cli/src/tui/fx.rs` 的 `FONT`（字母 E G L N O P T Y 的 5 行位图）与 `JOIN` 阴影连接表，
  移植自 ascii.rest `src/pieces/big-text.ts`。
- `fx::glint` 的斜带公式（`|x + slant·y − at| < width/2` 为核心，`< width/2 + 1` 为边缘）
  参照 ascii.rest `src/kit/fx.ts` 的 `glint`。
- `fx::scramble` 的噪声字符表取自 ascii.rest `src/pieces/scramble.ts`。
- 以上在源文件里有注释注明素材名和 MIT 版权，许可全文见 `crates/oo-cli/src/tui/THIRD_PARTY.md`。
  其余效果（dissolve 聚合、radar 揭示、skeleton 光带、sparkline、热力条、绽放粒子）只借思路，是重写。

## 5. 易用性底线（实现时逐条核对）

1. 动画只在 `Theme.motion` 为真时播放：`NO_COLOR`、`TERM=dumb`、`--no-anim`、`OO_NO_ANIM`、
   `OO_REDUCED_MOTION`、`REDUCE_MOTION` 都关掉它；非 TTY 和 `--json` 根本不进 TUI。
2. 一次性动画：开场 ≤ 1.2s、图谱揭示 0.8s、交付庆祝 1s；任意键立刻跳到静止帧（揭示中的第一个键只用于跳过）。
3. 静止帧 = 快照帧，所以 80×24 / 120×40 × 单色 / 彩色 的快照覆盖的就是关闭动画时用户看到的全部内容。
4. 装饰点只出现在开场和标题纹理；普通屏右下角的环境点阵删除。
5. 语言跟随 `OO_LANG`，其次 `LC_ALL`、`LANG`：以 `zh` 开头用中文，否则英文。框架、按键提示、状态词、理由全部双语。
