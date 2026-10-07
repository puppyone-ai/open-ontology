# FE-01 / G1 交接

本提交是 G1 基础里程碑的可审阅切片。FE-01 未完成。A01–A20、50 个页面、live 绑定、完整旅程和发布都未验收。G3 还没审这个提交。

## 已固定、后续可以直接依赖的接口

- pnpm workspace：`package.json`、`pnpm-workspace.yaml`、`pnpm-lock.yaml`。包管理器 `pnpm@12.10.1`，Node `24.21.0`。
- 脚本在 `@open-ontology/web`：`check`、`types`、`lint`、`test`、`build`、`test:e2e`、`test:visual`、`test:a11y`、`storybook`、`build-storybook`。`check` 不含 e2e。
- Token：`apps/web/design/tokens.json` 与 `apps/web/src/styles/tokens.css`。组件使用语义变量，不把原始色写进组件。
- 主题与密度：`<html data-theme="light|dark|system">`、`<html data-density="compact|comfortable|roomy">`。不放进假身份或 localStorage。
- 字体家族：`Geist`，然后 `Noto Sans SC`（子集 unicode-range），然后 `Noto Sans SC Full`，然后系统 CJK。`font-display: swap`。无外部字体 URL。
- 唯一 Radix 直接依赖：`radix-ui` 1.7.0。不要再加一套 `@radix-ui/react-*` 直接依赖。
- `@open-ontology/sdk-ts` 的包名和 `exports["."]` 指向 `./src/index.ts`。源文件还不存在。
- 预锁但本页不导入：React Router 8.4.0、TanStack Query 5.104.1、TanStack Table 9.2.6、React Hook Form 7.89.0、Zod 4.6.5、MSW 2.15.0。

## 仍由 G1 持有、下一里程碑再改的路径

- `apps/web/src/main.tsx` 现在是静态基线。路由组装替换它之前，要先有协调者的路径交接。不要在未交接时平行改这个文件。
- 锁、Token、字体和 `apps/web/config/**` 仍由 G1 单一写入。G2 不改这些文件。

## 队列

1. G1 下一固定里程碑：公共组件契约、`ModuleRegistration`、Shell 和 SDK 源文件。每个里程碑单独提交，并给 G2 一份类型化的接口说明。不要把未提交的大块 UI 留在工作区当交接。
2. G2 `eef831aa-dba4-4af2-a78b-541bc9e0a353` 继续预留。它要等上述共享接口进入固定提交后再启动。
3. G3 `eef831aa-dba4-4af2-a78b-541bc9e0a352` 继续预留。只审固定提交，不从本会话派生第二个审阅 UUID。

## 浏览器缺口

见 FINDINGS。在补上真实截图、axe、键盘、减少动效和字体网络记录之前，不能把这一页标成视觉或无障碍通过。

## 清理

只删除本任务的退出容器和空的 `oo-fe01-net-eef831aa`。不 prune，不清理别人的容器。结果写在 `raw/cleanup.txt`。
