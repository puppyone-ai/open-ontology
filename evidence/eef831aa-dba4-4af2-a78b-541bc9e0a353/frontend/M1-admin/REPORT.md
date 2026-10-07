# M1 管理页报告

角色：G2，UUID `eef831aa-dba4-4af2-a78b-541bc9e0a353`。这一刀只加总览、数据源、文件和任务。不签页面验收，不签 live、旅程或发行。

## 范围

`apps/web/src/features/admin-overview`、`sources`、`files`、`jobs`，以及 `apps/web/tests/e2e/admin/unmounted.spec.ts`。每页有 `module.ts` 注册，`layout` 是 `admin`，`capability` 写成 `admin.Axx`，这只是路由标记，不是服务端授权。

页面读取 `@open-ontology/sdk-ts` 的 `bindingById` 和 `invokeOperation`。登记仍是 unbound，调用结果是 `not_connected`。没有 fetch。生产路由没有改。`ADMIN_MODULES` 仍是空数组，`/admin` 仍进入 `/admin/login`。

## 已证明

最终容器 `oo-a353-node-m1c`。镜像 `docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413`。Node v24.21.0，pnpm 12.10.1。4GiB 内存、4GiB swap、1 CPU、pids 2048、`--cap-drop ALL`、`privileged=false`、网络 `none`。没有安装。锁是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。

核对过归档里的 `router.tsx`、`tokens.css`、`bindings.ts` 和锁与提交时的 HEAD 一致。管理页是覆盖进去的未提交文件。

| 阶段 | 退出码 |
|---|---|
| tsc app | 0 |
| tsc node | 0 |
| tsc sdk | 0 |
| eslint | 0 |
| vitest | 0。13 个文件，37 个测试。开始 22:48:49，耗时 10.33s |
| vite build | 0 |

生产包 `index-NwMOhte-.js` raw 325.77 kB、gzip 102.83 kB；`index-DlGqrOTS.css` raw 21.16 kB、gzip 5.29 kB。入口仍是 112 个模块。管理页没有被 G1 路由引用，所以不在这个包里。

## 失败尝试

1. `oo-a353-node-m1` 的类型检查退出码 2。测试文件用页面自己不可能出现的 extra 做比较，TypeScript 报 no overlap。Vitest 当时仍是 0。日志 `raw/failed/attempt-1-types.log`。
2. `oo-a353-node-m1b` 的脚本最后打印了 DONE，容器进程是 0，但阶段退出码是类型 2、测试 1、构建 1。刷新私有归档时删掉了整个 `features` 目录，G1 的 auth 和 user 文件一起没了。这不是产品缺陷。日志 `raw/failed/attempt-2/`。随后从 Git 归档把 G1 文件放回去，再覆盖 G2 目录。

## 未证明

浏览器没有打开。`unmounted.spec.ts` 不在 G1 的 Playwright 配置里，没有执行。没有 Rust。B01–B12 仍是 unbound。这 8 个页面没有真实数据、任务或文件。
