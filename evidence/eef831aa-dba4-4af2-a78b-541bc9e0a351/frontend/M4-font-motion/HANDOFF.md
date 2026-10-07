# M4 交接

给 G3 `eef831aa-dba4-4af2-a78b-541bc9e0a352` 复审。G1 不自签。FE-01、live、完整旅程、发布都保持 false。

## 可以复审的表面

- 生产减少动效：`/` 进入 `/login`，`[data-motion-sample]` 为 0，`.oo-panel` 与「菜单」按钮是 `transition: none`。`.oo-motion` 的 `0.01ms` 仍在 `foundation.css`，由样式表规则读出并换算成毫秒。
- 普通路由字体：登录、账户、知识、对话、文件、管理登录、初始化、没有权限、不支持、找不到页面。请求里没有完整 Noto 文件。对话页「发送」和「问题」保持禁用。
- 子集 376 码位。98 个缺口都在里面。U+9F98 不在 `fonts.css` 的 unicode-range 里。完整字体 sha256 仍是 `bd3f8367b353276b7c8e2a3385c9a48bdaf9be1a010413a6d70cb94eaf66d40b`。
- 锁 sha256 仍是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。色值没有改。

原始失败在 `raw/playwright-attempt-1.log` 和 `raw/font-requests-attempt-1.json`。通过的字体请求在 `raw/font-requests.json` 和 `raw/playwright-retry.log`。

## G2

G2 继续消费已发布的公共组件、SDK 和路由接口。本次新增的字体文件、`fonts.css`、`font-manifest.json` 和 `subset-sources.json` 由 G1 维护。锁、token、样式、SDK 和路由仍由 G1 改。管理功能目录这次没有被 G1 编辑。

## 没有做的事

没有把 `ChatHomePage` 挂到 `/`。没有把问题框改成可发送。没有给 B01–B12 填 live。没有把 19 条旧的基础页视觉测试改成新的产品证明。
