# M2 管理页报告

角色：G2，UUID `eef831aa-dba4-4af2-a78b-541bc9e0a353`。这一刀在已提交的总览、数据源、文件和任务之上，加构建、审核、分类和发布。不签页面验收，不签 live、旅程或发行。

## 范围

新增 `apps/web/src/features/builds`、`reviews`、`classifications`、`releases`。`admin-overview/catalog.ts` 和 `nav.ts` 追加 A11、A12、A17、A18、A19、A29、A30。已提交的 M1 页面文件没有改。

每页有 `module.ts` 注册，`layout` 是 `admin`，`capability` 写成 `admin.Axx`。这只是路由标记，不是服务端授权。页面读取 `@open-ontology/sdk-ts` 的 `bindingById` 和 `invokeOperation`。登记仍是 unbound，调用结果是 `not_connected`。没有 fetch。生产路由没有改。`ADMIN_MODULES` 仍是空数组，`/admin` 仍进入 `/admin/login`。

构建详情列出 S0 到 S10，徽章都是 `not_run`。审核决定表单保持禁用，候选文案是“没有候选”。发布详情的 15 个门禁都是 `not_run`，测试断言 `data-gate=pass` 的个数是 0。分类页写明 unknown 不是 0。

## 已证明

最终容器 `oo-a353-node-m2b`。归档源 `c3a43526e66a0245919e5b5932fb958570a7b7f5`，M1 提交 `f67a87b0a76b9f8d863957a7d41509e462e1c1da` 是它的祖先。镜像 `docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413`。Node v24.21.0，pnpm 12.10.1。4GiB 内存、4GiB swap、1 CPU、pids 2048、`--cap-drop ALL`、`privileged=false`、网络 `none`。没有安装。锁是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。

归档里的 `router.tsx`、`tokens.css`、`bindings.ts`、`module-registration.ts` 和锁与归档当时的工作区一致，哈希在 `raw/frozen-sha256.txt`。G1 的 auth 和 user 目录还在。管理页是覆盖进去的未提交文件。node_modules 从这一 UUID 的 M0 私有副本克隆，没有跑 `pnpm install`。

| 阶段 | 退出码 |
|---|---|
| tsc app | 0 |
| tsc node | 0 |
| tsc sdk | 0 |
| eslint | 0 |
| vitest | 0。17 个文件，41 个测试。开始 22:56:18，耗时 13.86s |
| vite build | 0 |

生产包 `index-DGD3fIPZ.js` raw 325.77 kB、gzip 102.83 kB；`index-GQYUKJ5S.css` raw 20.07 kB、gzip 5.14 kB；`index.html` raw 0.76 kB、gzip 0.37 kB。入口仍是 112 个模块。管理页没有被 G1 路由引用，所以不在这个包里。

## 失败尝试

`oo-a353-node-m2b` 没有失败阶段。更早的 `oo-a353-node-m2` 在源 `f67a87b0a76b9f8d863957a7d41509e462e1c1da` 上同样是类型、lint、测试、构建退出码 0（17 个文件，41 个测试，开始 22:51:07，耗时 14.32s）。其后 HEAD 到了 `c3a43526e66a0245919e5b5932fb958570a7b7f5`，这段历史里 `apps/web` 和 `packages/sdk-ts` 没有差异。本报告以 `oo-a353-node-m2b` 为准。

## 未证明

浏览器没有打开。`apps/web/tests/e2e/admin/unmounted.spec.ts` 不在 G1 的 Playwright 配置里，没有执行。没有 Rust。B01–B12 仍是 unbound。这 7 个页面没有真实构建、审核对象、分类清单或发布。生产包不包含它们。
