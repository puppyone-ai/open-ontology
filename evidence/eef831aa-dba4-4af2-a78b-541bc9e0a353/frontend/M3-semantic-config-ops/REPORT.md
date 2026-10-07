# M3 管理页报告

角色：G2，UUID `eef831aa-dba4-4af2-a78b-541bc9e0a353`。这一刀补上其余管理页：本体、码表和日历、指标、实体、示例、权限、模型与 Secret、设置和渠道、评测、审计与会话、运维。不签页面验收，不签 live、旅程或发行。

## 范围

新增 `ontology-catalog`、`enums-calendar`、`metrics`、`entities-terms`、`examples`、`permissions`、`models-secrets`、`settings-integrations`、`evaluations`、`audit-sessions`、`operations`。`admin-overview/catalog.ts` 和 `nav.ts` 追加 A13–A16、A20–A28、A31–A43。已提交的 M1、M2 页面文件没有改。

每页有 `module.ts` 注册，`layout` 是 `admin`，`capability` 写成 `admin.Axx`。这只是路由标记，不是服务端授权。页面读取 `@open-ontology/sdk-ts` 的 `bindingById` 和 `invokeOperation`。登记仍是 unbound，调用结果是 `not_connected`。没有 fetch。生产路由没有改。`ADMIN_MODULES` 仍是空数组，`/admin` 仍进入 `/admin/login`。

模型页和 Secret 页没有放 `SecretInput`。恢复向导的 `canSubmit` 是 false，`planVersion` 是 null。指标详情把可执行、对账、owner 认证、已发布四项都写成尚未接入。运维页写明 `gateway.health.get` 不是这一页的 B12 绑定。

## 已证明

容器 `oo-a353-node-m3`。归档源 `7151fcbb7aa0bc8ad9bfb6c4195bd8e66efcfee1`。镜像 `docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413`。Node v24.21.0，pnpm 12.10.1。4GiB 内存、4GiB swap、1 CPU、pids 2048、`--cap-drop ALL`、`privileged=false`、网络 `none`。没有安装。锁是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。

归档里的 `router.tsx`、`tokens.css`、`bindings.ts`、`module-registration.ts` 和锁与归档当时的工作区一致，哈希在 `raw/frozen-sha256.txt`。G1 的 auth 和 user 目录还在。管理页是覆盖进去的未提交文件。node_modules 从这一 UUID 的 M0 私有副本克隆，没有跑 `pnpm install`。

| 阶段 | 退出码 |
|---|---|
| tsc app | 0 |
| tsc node | 0 |
| tsc sdk | 0 |
| eslint | 0 |
| vitest | 0。28 个文件，52 个测试。开始 22:59:53，耗时 22.16s |
| vite build | 0 |

生产包 `index-DGD3fIPZ.js` raw 325.77 kB、gzip 102.83 kB；`index-GQYUKJ5S.css` raw 20.07 kB、gzip 5.14 kB；`index.html` raw 0.76 kB、gzip 0.37 kB。入口仍是 112 个模块。管理页没有被 G1 路由引用，所以不在这个包里。

## 失败尝试

`oo-a353-node-m3` 没有失败阶段。

## 未证明

浏览器没有打开。`apps/web/tests/e2e/admin/unmounted.spec.ts` 不在 G1 的 Playwright 配置里，没有执行。没有 Rust。B01–B12 仍是 unbound。这些页面没有真实本体、权限、模型、评测、审计或恢复。生产包不包含它们。
