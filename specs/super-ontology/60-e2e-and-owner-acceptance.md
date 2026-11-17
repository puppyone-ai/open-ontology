# Super Ontology 端到端测试与 owner 验收

> 前提：[00 决策记录](00-overview-and-decisions.md) D40；[30 闭环与 Sprint](30-closed-loops-and-sprints.md)；[20 前端](20-frontend.md) 的测试方案。
> 本文规定的是：owner 拿到一个本地打包好的 App，直接点着用新功能；在交给 owner 之前，这个 App 必须先通过一整套用真实点击完成的端到端测试。

> 执行顺序修订（D80）：阶段一（OO 先行）期间，L4、L6 交付 CLI 验收包，打包 App 的 owner 验收包与 T1/T2/T3 在阶段二按闭环补齐，见 30 §7。

## 1. 原则

1. **测的是要交付的那个 App。** 最终验收的对象是 `npm run dist:mac` 产出的 `.app`/`.dmg`，不是 dev server，也不是 `vite build` 加裸 Electron。打包后才会暴露的问题（asar 路径、资源缺失、签名与 Gatekeeper、主进程找不到 `oo`、权限弹窗）只有这样才测得到。
2. **点击是真的。** 关键路径必须用操作系统级的输入事件完成（鼠标移动、按下、抬起、拖拽、键盘），而不只是 DOM 上的 `click()`。Puppyone 仓库对此已经有明确的立场和工具：`tests/README.md` 写明 “CDP file selection and synthetic DOM events are not OS gesture certification”；`tests/support/electron/macos-native-pointer.c`、`macos-native-input.c` 用 CGEvent 发真实事件。
3. **后端是真的。** 端到端测试连真实的 `oo serve`、真实的 PG、真实的 parser sidecar、真实的 RustFS，不用 mock。LLM 用两种配置各跑一遍：不配模型（只跑确定性规则）、配测试模型（经 `secret_ref` 引用 `~/.config/open-ontology/secrets/` 下的文件）。
4. **从零开始。** 每次运行都用全新的用户数据目录和全新的 Drive，模拟第一次安装。
5. **证据先于结论。** 每一步留下截图，关键流程留下录屏，失败时自动保存日志与界面状态。没有证据就不算通过。

## 2. 测试方法调研结论

| 方法 | 能做什么 | 限制 | 结论 |
|---|---|---|---|
| **仓库现有的 Electron 场景脚本**（`tests/e2e/**/*.smoke.mjs`，由 `tests/support/runners/run-desktop-e2e.mjs` 用 Electron 二进制直接运行；`--native` 时用 CGEvent 发真实点击，并用原生截屏校验像素） | 在主进程里拿到 `BrowserWindow`，可以精确算出元素的屏幕坐标再发真实点击；断言既能读 DOM，也能读主进程状态 | 跑的是源码构建，不是打包产物；`--native` 需要 macOS 的辅助功能和屏幕录制权限，GitHub 托管机上拿不到 | **T1 层的主力**，和现有测试同一套写法 |
| **Playwright 驱动打包后的 App**（[Electron 支持](https://playwright.dev/docs/api/class-electron)，仍标为 experimental） | 用 `_electron.launch({executablePath})` 启动打包后的二进制，拿到主进程和页面 | 依赖 `EnableNodeCliInspectArguments` fuse 处于打开状态，关闭后启动会超时（[podman-desktop #20065](https://github.com/podman-desktop/podman-desktop/pull/20065)）。另一种做法是用 Chromium 的 `--remote-debugging-port` 启动打包 App，再 `chromium.connectOverCDP` 连上，这个开关不受 fuse 控制，代价是拿不到主进程 API | **T2 层用 `connectOverCDP` 做断言和读状态**。目前 Puppyone 没有配置 fuse（`electron/main.mjs:230` 只在 dev 时开 9222 端口），将来如果关掉 fuse，这条路依然可用 |
| [@wdio/electron-service](https://webdriver.io/docs/wdio-electron-service)（原 [wdio-electron-service](https://github.com/webdriverio-community/wdio-electron-service)，已迁到官方组织） | 自动找到 electron-builder 的产物路径，支持打包后的 App | 要引入整套 WebdriverIO；点击仍然是 WebDriver 层的合成事件 | 不采用：和仓库现有写法不一致，也不解决“真实点击”的问题 |
| **[Peekaboo](https://github.com/openclaw/Peekaboo)**（macOS CLI + MCP，[文档](https://peekaboo.sh/)） | 读取任意 App 的辅助功能树，按标签点击、输入、拖拽、滚动；截屏；`.peekaboo.json` 脚本；还有 Agent 模式 | 需要 macOS 15+，并授予辅助功能、屏幕录制、事件合成权限；Electron App 的辅助功能树质量取决于 ARIA 标注是否完整 | **T2 层的操作手**：按可访问名称点击打包后的 App，完全黑盒。同时倒逼前端把 ARIA 标注做全（这本来就是 20 分册的要求） |
| **Agent 探索式测试**（Claude 通过 Peekaboo MCP 或本仓库已有的 `proofshot` 技能，按任务描述像用户一样操作，全程录屏） | 能发现脚本想不到的问题：文案看不懂、找不到入口、状态卡住 | 结果不稳定，不能作为通过与否的判据 | **T3 层**：作为必交证据，不作为阻断门禁 |
| macOS XCUITest | 原生 App 的官方方案 | 不适合 Electron | 不采用 |

## 3. 四层测试

```
T1  源码构建 + 真实后端 + 真实点击（仓库内场景脚本）      每个闭环签收前必须全绿
T2  打包 App + 真实后端 + 黑盒真实点击（Peekaboo + CDP）    每个闭环签收前必须全绿
T3  打包 App + Agent 探索式操作（录屏 + 问题清单）          每个闭环签收前必须提交，不阻断
T4  owner 本人在打包 App 上验收                              里程碑结束时（L2 可选，L4、L6、L8 必须）
```

### 3.1 T1：仓库内场景（Puppyone `tests/e2e/super-drive/`）

- 写法与现有 `*.smoke.mjs` 一致，注册进 `run-desktop-e2e.mjs` 和 `scripts/release-checks/checks.json`。
- 后端由测试前置脚本拉起：`oo serve`（由它管理文档引擎的 compose 项目），所有容器打 `oo.lane=super-ontology-e2e` 标签，测试结束全部删除。测试 Drive 用评测集 `testing/super-ontology/golden/` 的副本。
- 关键路径必须带 `--native`，用真实点击：
  - 拖拽上传（原生拖拽，参考 `tests/support/electron/native-resource-acceptance.mjs`）；
  - 列表多选与右键菜单；
  - 失败横幅 → 筛选 → 批量重试；
  - 待确认队列“按来源整体接受”；
  - 分享面板：选范围 → 选角色 → 改预算 → 一键执行 mount；
  - 撤销确认。
- 其余场景可以用 DOM 层操作加快速度，但每个场景至少有一次原生点击，用来证明真实输入链路是通的。

### 3.2 T2：打包 App 黑盒（Puppyone `tests/e2e/super-drive/packaged/`）

**被测对象**：本次要交付的 `.app`，由验收构建渠道 `acceptance` 打包（应用名 `PuppyOne Acceptance`、bundle id `ai.puppyone.desktop.acceptance`、数据目录 `puppyone-acceptance`，不检查更新；见 20 分册 §1.10.2）。`--user-data-dir` 会被 `build-info-service.mjs` 覆盖、不生效，所以用只在 acceptance 与 dev 渠道生效的 `--puppyone-user-data-dir`。在 `dist:mac` 之后、交给 owner 之前测。记录它的 sha256，后面交付的必须是同一个文件。

**启动方式**：
```bash
# 全新用户数据目录，模拟首次安装；开启 CDP 端口只用于读取状态和断言
"$APP/Contents/MacOS/PuppyOne Acceptance" --puppyone-user-data-dir="$RUN/profile" --remote-debugging-port=0
```
- **操作**一律经 Peekaboo 发出（按可访问名称点击、输入、拖拽），不经 CDP。
- **断言**两路并行：经 CDP 读 DOM 和应用状态，经 Peekaboo 截屏比对关键区域。
- 每一步截屏，整轮录屏；失败时保存 Peekaboo 快照、渲染进程的 console、主进程日志、`oo serve` 日志。

**必须覆盖的场景**（每个场景都从用户视角写成步骤，放在 `tests/e2e/super-drive/packaged/scenarios/*.peekaboo.json`）：

| # | 场景 | 关键断言 |
|---|---|---|
| P1 | 首次启动，未装 Docker（用 PATH 隔离模拟） | 出现安装引导；不崩溃；装好 Docker 后点“重试”能继续 |
| P2 | 首次启动，启动本地引擎 | 进度可见；引擎状态变为健康 |
| P3 | 打开 flag，进入 Super Drive，空 Drive | 空态与“导入示例文件夹”可用 |
| P4 | 登记本地文件夹为 Drive，拖入评测集各种格式 | 每个文件走完状态；队列面板数字正确 |
| P5 | 拖入不支持的格式、超大文件、加密文件、毒丸文件 | 各自进入正确的终态，文案可读，其他文件不受影响 |
| P6 | 部分失败 → 横幅 → 筛选 → 批量重试 | 重试后状态正确 |
| P7 | 处理中杀掉 parser sidecar 容器 | 界面显示降级；容器恢复后自动继续 |
| P8 | 退出 App 再打开 | 状态、队列、Share 全部保留；事件不丢 |
| P9 | 待确认队列：按种类筛选、按来源整体接受、拒绝 | 接受后实体详情更新，本体版本加一 |
| P10 | 点开文件 → 解析结果 → 点一个锚点 | 预览定位到原文位置 |
| P11 | 分享面板：选范围、默认角色、预算、任务描述 → 一键执行 | 目标目录出现 `.context/`；BRIEF 每行带出处；内容与命令行编译的逐字节一致 |
| P12 | 红队：Drive 里放红队文件，共享一个不包含它的范围 | `.context/` 下全文搜索不到任何金丝雀字符串 |
| P13 | 撤销 Share | 确认框写明“已读内容无法收回”；挂载目录被删除；`refs/` 凭证失效 |
| P14 | 未配置模型 | 横幅出现；仍能建出确定性的 Index 并完成 P11 |
| P15 | 断网（关闭 loopback 之外的网络） | 本地功能全部可用 |
| P16 | 切换到英文界面重跑 P3、P4、P11 | 没有缺失的翻译 key，没有截断 |
| P17（L5 起） | Agent 写建议 → 修订模式里批量接受 | 下一次 BRIEF 内容变化 |
| P18（L4 起；L6 加差异摘要） | 修改被引用的文件 → Share 漂移 → 刷新 | 5 秒内显示“有更新”；刷新后生成新 Share；L6 起 BRIEF 头部列出差异 |
| P19（L3 起） | 不经过 App，直接在访达里往 Drive 放入、修改、删除文件 | 状态自动更新；新鲜度指示从“正在处理”回到“已是最新 · vN” |
| P20（L3 起） | 单文件延迟：分别放入 md、20 页 PDF、XLSX，计时到 `published` | 达到 D49；耗时取自 G13 埋点，与界面显示一致 |
| P21（L4 起） | 两个并行 Agent：用 `--group` 为同一 ticket 挂两个 Share；在 Desktop 里把其中一个 Share 用到的决定标记为错误 | 两个 Share 版本相同；被用到的那个在 5 秒内出现 `block` 级更新，`.context/UPDATES.md` 只追加；模拟 Agent 在提交前执行 `oo context check` 返回 20，提交被拦住；整组刷新后两个 Share 同时升级，check 返回 0；`.context/` 其余文件哈希全程不变 |
| P22（L6 起） | 拖入一段 H.264/AAC 的 mp4 会议录屏和一段 m4a 录音（macOS） | 经宿主机解码处理成功；进度按时间显示；转写按说话人分段；点击出处从对应时间点播放 |
| P23（L6 起） | 说话人确认：把 `S1` 确认为“小王” | 确认前该说话人的内容不进入本体；确认后事实主语为小王；界面没有任何声纹库或跨录音识别入口 |
| P24（L6 起） | 带 PPT 翻页的录屏视频 | 每页幻灯片一个关键帧；帧文字可搜索；出处 `#f=` 可跳转 |
| P25（L4 起） | 打开 BRIEF 预览、打开 `.context/` 下的生成文件、打开“关于 → 开源许可” | 首行“AI生成”标识与 front matter 存在且无法在产品内关闭；开源许可页列出全部组件 |
| P26（L2 起；L4、L7 扩展） | 依次在默认、登记 S3 桶、配置第三方模型三种状态下查看 Super Drive 底部与设置页的数据说明，同时抓包确认实际出网 | 说明文字与实际数据流一致：默认状态下除 loopback 外无出网；配置模型后只出现到所选服务商的连接；文案随配置变化 |

**运行环境**：owner 的这台 Mac，或者一台专用的自托管 macOS 机器。需要事先给终端和 Peekaboo 授予辅助功能、屏幕录制、事件合成权限，GitHub 托管机做不到这一点，所以 T2 是**本地门禁**，结果以证据目录为准，不在 CI 上跑。运行期间不要碰鼠标和键盘。

### 3.3 T3：Agent 探索式测试

- 用一个 Agent（Claude 加 Peekaboo MCP，或 `proofshot` 技能）拿到一段用户视角的任务，例如：“你是一个新用户，把这个文件夹里的资料交给 Agent 去跟进项目 A，并确认它没有看到客户 B 的资料”。
- 不给 Agent 看测试脚本，只给 App 和任务。
- 产出：完整录屏、操作日志、问题清单（每条写清卡在哪、截图、严重程度）。
- 问题清单里的“阻断”级问题必须在交付 owner 之前修掉；其他问题进下一个 Sprint 的任务卡。

### 3.4 T4：owner 验收包

在 T1、T2 全绿、T3 没有阻断级问题之后，开发者交付一个验收包，放在 `~/Desktop/super-ontology-acceptance/L<n>/`（不进仓库）：

| 内容 | 说明 |
|---|---|
| `PuppyOne-super-drive-L<n>.dmg` | 就是 T2 测过的那个文件，sha256 一致 |
| `BUILD.txt` | 两个仓库的 commit sha、`oo` 版本、契约版本、打包时间、T1/T2 运行编号 |
| `sample-drive/` | 一份示例资料（评测集的子集，加几份贴近日常的文件）；同一份资料也作为 `extraResources` 打进验收渠道的包，供“导入示例文件夹”离线使用，正式渠道不带 |
| `ACCEPTANCE.md` | 15–20 步、约 20 分钟的验收清单，每一步写“做什么”和“应该看到什么”，见 §4 |
| `evidence/` | T2 的截图与录屏、T3 的录屏与问题清单 |
| `KNOWN-ISSUES.md` | 已知但不阻断的问题 |
| `FEEDBACK.md` | 反馈模板：步骤号、看到了什么、期望什么、截图 |

**安装说明写在 `ACCEPTANCE.md` 第一步**：打包时关闭了签名（`CSC_IDENTITY_AUTO_DISCOVERY=false`），首次打开要右键 →“打开”，或者在“系统设置 → 隐私与安全性”里允许。验收版和 owner 日常使用的正式版共存：验收版用独立的应用名和用户数据目录，不碰正式版的数据。

**owner 不需要打开终端**：引擎由 App 启动，示例资料一键导入，`oo context mount` 由分享面板的“一键执行”完成，结果目录在访达里打开查看。唯一的前置条件是本机已装好 Docker（第一版的要求，D30）。

## 4. `ACCEPTANCE.md` 模板（M1，L4）

1. 安装并打开 App（右键 → 打开）。**应看到**：正常启动，没有报错。
2. 设置 → 实验功能 → 打开 Super Drive。**应看到**：如果还没打开任何项目，入口页上出现“打开 Super Drive”；已有项目窗口时，侧栏出现 Super Drive。
3. 进入 Super Drive。**应看到**：提示启动本地引擎；点击后有进度，约 1 分钟内变为“健康”。
4. 点“导入示例文件夹”。**应看到**：文件逐个从“排队”走到“已发布”，右下角队列面板数字递减。
5. 找到标红的失败文件（示例里故意放了一个损坏的 PDF）。**应看到**：顶部横幅写明失败数量；点开能看到人话的原因。
6. 点开一份会议纪要。**应看到**：右侧详情栏列出识别出的人和项目；点一个证据能跳到原文位置。
7. 打开待确认队列。**应看到**：按来源分组的条目；对一个来源点“整体接受”，一次完成。
8. 在实体里搜索一个人的别名（示例里“小王”）。**应看到**：解析到同一个人。
9. 右键 `Projects/项目A` → 共享给 Agent。**应看到**：默认角色是“可建议”，有效期 7 天，可以填任务描述和预算。
10. 填任务描述“跟进方案二”，点“一键执行”，目标选一个空文件夹。**应看到**：访达打开，里面有 `.context/BRIEF.md`。
11. 打开 BRIEF.md。**应看到**：第一行写明覆盖情况（例如“覆盖：42/42”）和一致性承诺摘要；篇幅在预算内；有“相关的人”“当前状态与决定”“已试过但走不通”；每行末尾有出处。
12. 检查 BRIEF 末尾的“已拦截”。**应看到**：只有数量，没有客户 B 的任何名字。
13. 回到共享列表。**应看到**：刚才的共享，状态“生效中”。
13a. 在实体详情里把 BRIEF 中出现的一个决定“标记为错误”。**应看到**：几秒后共享列表里这个共享显示红色“有更新”；`.context/UPDATES.md` 多出一条记录，其余文件不变；点“刷新”后生成新的共享，旧的显示为“已刷新”。
14. 撤销它。**应看到**：确认框写明“Agent 已读内容无法收回”；确认后目录被删除，状态变为“已撤销”。
15. 退出 App 再打开。**应看到**：所有状态都还在。
16. （可选）设置里配置模型后，点“重新抽取”。**应看到**：识别出的实体变多，新增的低置信条目进入待确认队列。

L6、L8 的清单在对应闭环开始时补写，追加 Suggest、漂移刷新、OCR、图片、会议录音与录屏（含说话人确认）、企业桶、免 Docker 的步骤。验收包不附用户须知（D68，用户须知由官网发布）。

## 5. 与闭环的关系

| 闭环 | T1 | T2 | T3 | T4 owner 验收包 |
|---|---|---|---|---|
| L1 | flag 关闭的回归 | — | — | — |
| L2 | 骨架场景 | P2–P4（只有 Markdown）、P8、P11、P26（默认状态） | 可选 | 可选（预览版） |
| L3 | 解析与状态场景 | P1–P8、P10、P15、P16、P19、P20 | 必交 | — |
| L4 | 全部第一版场景 | P1–P16、P18（漂移提示）、P19–P21、P25、P26 | 必交 | **必须** |
| L5 | + Suggest | + P17 | 必交 | — |
| L6 | + 多模态、差异摘要与 OCR | + P18（差异摘要）、P22–P24 | 必交 | **必须** |
| L7 | + 企业桶、Cloud 代理 | + 对应场景 | 必交 | — |
| L8 | + 免 Docker、规模 | 全量，含 1 万文件 Drive | 必交 | **必须** |

## 6. 前端的配合要求

T2 黑盒点击依赖辅助功能树，所以这些要求从 L2 起就是前端的验收项（写进 20 分册的任务卡）：

- 每个可交互元素都有稳定的可访问名称（`aria-label` 或可见文本），并且在 8 种语言下各自正确。
- 状态徽标、横幅、队列面板的数字用 `role="status"` 或 `aria-live`，让 Peekaboo 和读屏软件都能读到。
- 拖拽目标区域有可访问名称；右键菜单项可以通过辅助功能树找到。
- 另给一个只用于测试的 `data-testid`，T1 和 T2 的 CDP 断言用它定位；但 T2 的**操作**不得用 `data-testid`，必须走可访问名称，保证测的是用户能看到的东西。

## 7. 新增门禁

| 门禁 | 引入于 | 性质 |
|---|---|---|
| T1 `super-drive` 场景全绿（含 `--native`） | L2 | 阻断签收 |
| T2 打包 App 场景全绿，证据目录完整，交付文件 sha256 与被测文件一致 | L2 | 阻断签收与交付 |
| T3 没有未处理的阻断级问题 | L3 | 阻断交付 owner |
| 可访问名称覆盖率：Super Drive 的所有可交互元素 100% | L2 | 阻断签收 |
