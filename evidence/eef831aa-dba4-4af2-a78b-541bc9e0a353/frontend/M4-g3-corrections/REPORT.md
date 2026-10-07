# M4 复审修正报告

角色：G2，UUID `eef831aa-dba4-4af2-a78b-541bc9e0a353`。这一刀只改 G3 在 `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a352/frontend/G2-admin/HANDOFF.md` 里点名的三处。不签页面验收，不签 live、旅程或发行。

## 改动

`apps/web/src/features/admin-overview/blocks.tsx`：`SOURCE_STEPS` 的「连接」步骤还在，标题仍是「连接」，名称字段仍是文本。`kind: "secret"` 的 Secret 字段已去掉。`B07.secretWrite` 仍是 unbound。共享 `Wizard` 没有改。

`ReviewDesk` 在没有真实候选时不再渲染 `ReviewDecisionForm`。页面留下「没有候选」和空证据面板「没有可显示的证据。」共享 `review-decision-form.tsx` 没有改。

`apps/web/src/features/admin-overview/nav.ts`：12 条 `to` 含 `:` 的链接已去掉。导航剩 29 项，没有占位 `:id`。A06、A08、A10、A12、A14、A18、A20、A23、A24、A30、A38、A40 的 `module.ts` 注册还在。`AppShell` 没有改。

测试跟着这三处改了：`modules.test.tsx`、`sources/feature.test.tsx`、`reviews/feature.test.tsx`。模型页和 Secret 页仍然没有 `SecretInput`。

## 已证明

检查用的是固定提交 `d416874eee65e5f30ad9858cef4429f608066c7a` 的归档，再覆盖上面 5 个文件。工作区里的 `router.tsx` 与这份归档不一致，归档保留了固定提交的路由，没有抄工作区。`module-registration.ts`、`tokens.css`、`bindings.ts`、`wizard.tsx`、`review-decision-form.tsx`、`app-shell.tsx` 和锁与 `git show d416874ee` 逐字节一致，哈希在 `raw/frozen-sha256.txt`。

容器 `oo-a353-node-m4`。镜像 `docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413`。Node v24.21.0，pnpm 12.10.1。4GiB 内存、4GiB swap、1 CPU、pids 2048、`--cap-drop ALL`、`privileged=false`、网络 `none`。没有安装。锁是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。node_modules 从这一 UUID 的 M0 私有副本克隆。

| 阶段 | 退出码 |
|---|---|
| tsc app | 0 |
| tsc node | 0 |
| tsc sdk | 0 |
| eslint | 0 |
| vitest | 0。28 个文件，52 个测试。开始 23:57:44，耗时 23.84s |
| vite build | 0 |

生产包 `index-DGD3fIPZ.js` raw 325.77 kB、gzip 102.83 kB；`index-GQYUKJ5S.css` raw 20.07 kB、gzip 5.14 kB；`index.html` raw 0.76 kB、gzip 0.37 kB。入口仍是 112 个模块。管理页没有被这份归档里的路由引用，所以不在这个包里。

Rust 和 Clippy 没有跑。这一刀没有 Rust。

## 失败尝试

`oo-a353-node-m4` 没有失败阶段。

## 未证明

浏览器没有打开。Playwright 没有执行。B01–B12 仍是 unbound。没有真实候选、数据源 Secret 或详情对象。生产路由没有挂上这些页面。文件向导的文件标签和恢复向导的负责人仍是可输入文本，提交保持禁用；G3 这次没有要求改它们。
