# FE-01 / G1 发现

## 浏览器证明保留

页面截图、axe、键盘焦点和减少动效没有执行。不能把构建成功写成视觉或无障碍通过。

已下载的 Chromium 没有对页面启动。两次安装系统库都失败，容器自身退出码都是 4：

1. `oo-fe01-node-verify-3`：`playwright install-deps` 在 `--cap-drop ALL` 下执行 `setgroups` / `setegid`，返回 Operation not permitted。日志 `raw/playwright-install-deps.log`，退出文件 `raw/playwright-install-deps.exit`。
2. `oo-fe01-node-browser`：设置 `APT::Sandbox::User "root"` 之后，`apt-get update` 成功，安装阶段不能写入 `_apt` 拥有的 `/var/cache/apt/archives/partial`。`chmod` 返回 Operation not permitted，打开 deb 返回 Permission denied。日志 `raw/browser-libs.log`，退出文件 `raw/browser-libs.exit` 为 100。控制台 `raw/browser-attempt-console.log`。

没有添加 `--privileged`、host 网络、docker socket 或宿主 HOME。没有改用另一份未审镜像。

下一次只补浏览器时，可以在同一安全边界把 apt 缓存目录指到任务私有且由 root 拥有的路径，或者使用已经过审、预先装好 Debian 12 Chromium 库的镜像。不要重跑字体、锁、已通过的单元测试、类型检查、lint、对比度、生产构建和 Storybook，除非这些输入又改了。

## 失败尝试，均已保留

| 尝试 | 结果 |
|---|---|
| Rust 身份冒烟第一次 | 退出 127。`HOME=/tmp` 的登录 shell 丢掉了镜像 PATH。随后用显式 PATH 重跑，退出 0 |
| Node 引导 1 | 退出 3。在仓库目录探测 pnpm 时写出了只有 packageManager 的锁。已删除该桩锁，不当作既有基线 |
| Node 引导 2 | msw 3.0.2 不满足 `@vitest/mocker` 的 `msw ^2.4.9`。改为范围内的 2.15.0 |
| Node 引导 3 | pnpm 12 拒绝 msw 安装脚本。`allowBuilds.msw` 设为 false |
| Node 引导 4 | 退出 0。冻结安装、类型检查、lint、占位测试和构建通过 |
| 基础容器 | 字体和 `tsc` 为 0。lint 因 `build-fonts.mjs` 的 `no-undef` 和未使用变量失败 |
| verify | lint 为 0。Vitest 把 CSS `?raw` 读成空字符串 |
| verify-2 | 类型检查和 lint 为 0。`import.meta.url` 不是 `file:`，`readFileSync` 失败 |
| verify-3 | 单元 6、类型检查、lint、对比度、构建、Storybook、浏览器下载为 0。系统库安装失败 |
| browser | 只安装系统库。apt 缓存权限失败。退出 4 |

单元测试最终改为从包目录读取 `src/styles/tokens.css` 和 `fonts.css`。这次修改之后的类型检查、lint 和 6 个测试在 verify-3 通过，没有再跑字体或重装依赖。

## 其他边界

- 装饰边框不要求 3:1。它们留在 `contrast-tokens.json` 的 `decorative`，不拿来当控件边界。
- 暗色主按钮前景是 `#101318`，不是共用的白字。
- 主题和密度只写在 `documentElement` 的 `data-theme` / `data-density`。单元测试确认没有写入 `localStorage`。浏览器里的同一断言还没跑。
- 生产包没有导入已锁定的路由、Query、Table、MSW 或 Radix。这些包留给后续里程碑。
- `packages/sdk-ts` 没有实现文件。FE-03 之前，对它执行 `tsc` 会因没有输入而失败。
- 仓库 `.gitignore` 不包含 `node_modules`。本次提交使用显式路径，没有 `git add -A`。
- Recharts、React Flow、ELK 没有冻结。以后要加，必须回到 G1 改锁。
- 工作区 Rust 测试和 Clippy 是 `not_run`。
- a350 的文件和容器不属于本角色，没有修改。
- `specs/frontend/execution-status.md` 由 Codex 维护，本提交没有改它。
