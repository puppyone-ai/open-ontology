# M11 当前会话、初始化和退出

角色：G2，UUID `eef831aa-dba4-4af2-a78b-541bc9e0a353`。这一刀消费 G1 M12 固定源 `b12b5b41924754d1683c92d5a34ec1f49767839f`（`frontend: send same-origin calls with the current session`）。例程走真实的 `getIdentityCurrent`、`/admin/bootstrap` 的 `consumeBootstrapToken` 和管理壳的 `logoutManagementSession`。容器网络是 `none`，发行者没有在这台容器里运行。脚本里的响应体按合同形状填写，用来核对方法、路径和 CSRF。这不是发行者验收，也不是管理页或前端目标完成。

## 改动

固定源的 `callOperation` 在登记同源 https 传输之后发送 fetch。产品页面不传入 `fetchImpl`。G2 没有改 SDK、路由、字体、eslint、身份客户端或共享组件。

启动前没有名为 `oo-a353` 的容器，也没有 Node 容器。正在运行的 `oo-a383-rust`、`oo-a383-pg`、`oo-a373-pg` 没有动。

这一刀改了两个自有文件：

- `apps/web/src/features/admin-overview/session-example.test.tsx` sha256 `986446735509e8716d0834fbe0d38df426e9a9929f7e021da18cea8552bea51f`。第一条从 `/admin` 进入未初始化会话，提交引导令牌，看到「已经初始化。客户集成 not_claimed。」，进入总览 `fixture-admin · alpha · admin`，打开角色页，再点「退出」回到「管理登录」。记录到的请求依次是 current、bootstrap consume、current、logout、current。角色页显示「B06.roles 预检 not_precise」，「应用」禁用，页面上没有 `B01.projectRoleBinding` 预检行。第二条在 `https://admin.example` 上用测试专用 `fetchImpl` 调用 `callOperation`：`B01.projectRoleBinding` 与 `registerSource` 是 `not_precise` 且不发请求；随后 current、consume、current、logout、401 current、`listBuilds`。`listBuilds` 返回 404 纯文本，结果是 `invalid_schema`，`fetched` 为 true，`live`、`journey`、`release` 为 false。
- `apps/web/src/features/permissions/APIBINDING.md` sha256 `a9dbfcdc87395a62e36ae9632a9b4b343d8a2f81619573fb0f9a8e42508a8538`。记下上述固定源。角色绑定仍然没有写入接口。

M4 三处、M7 的当前会话和退出、M8 的角色守卫、M9 的十五条目录行、M10 的 `prepareCall` 预检都还在。页面按钮没有改为可点。

## 已证明

检查用的是 `b12b5b41924754d1683c92d5a34ec1f49767839f` 的 `apps/web`、`packages/sdk-ts`、锁、`tools/control-identity` 和 `tools/selfbuild-control-api`，再覆盖上面两个文件。`router.tsx`、`admin-gate.tsx`、`identity-session.ts`、`identity-context.tsx`、`bindings.ts`、`client.ts`、`eslint.config.mjs`、`b01-adapter.mjs`、`fonts.css`、字体清单、`app-shell.tsx`、`review-decision-form.tsx`、`blocks.tsx`、`nav.ts`、`admin-frame.tsx`、`pnpm-lock.yaml` 与固定提交逐字节一致，`frozen_cmp=ok`。锁是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。

容器 `oo-a353-node-m11`。镜像 `docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413`。Node v24.21.0，pnpm 12.10.1，aarch64，容器内 uid 0。4GiB 内存、4GiB swap、1 CPU、pids 2048、`--cap-drop ALL`、`privileged=false`、网络 `none`。没有安装。node_modules 从这一 UUID 的 M0 私有副本克隆。

| 阶段 | 退出码 |
|---|---|
| tsc app | 0 |
| tsc node | 0 |
| tsc sdk | 0 |
| eslint | 0。日志为空 |
| vitest | 0。34 个文件，79 个测试。日志 `Start at 08:14:09`，耗时 31.25s |
| docker | 0 |

`session-example.test.tsx` 2 个测试，357ms。第一条 353ms。第一次归档即完成。

Rust 和 Clippy 没有跑。生产包这次没有重新构建。浏览器没有打开。

## 未证明

运行时 origin 仍是 null。`live`、`journey`、`release` 仍是 false。B04 ready 仍是 false。`listBuilds` 的 404 是 `invalid_schema`，页面仍不调用它。十五条目录行仍是 `schema_frozen`。

`B01.projectRoleBinding` 没有写入接口。应用、保存、预览保持禁用。`registerSource` 在这条固定客户端上是 `not_precise`，不发请求。源、构建、发布生产者的服务没有接到管理页。

`IdentityView` 仍只有解析后的会话。页面不伪造 current 信封。jsdom 是 http，产品路径仍走相对 `/control/v1/`。https 例程只存在于测试里的 `fetchImpl`。

证明时 `raw/source-head.txt` 记录工作区 `77dcb07b443337eb216e044e19839b85020a0381`，`m12_ancestor=yes`。这次提交的父提交是 `77dcb07b443337eb216e044e19839b85020a0381`。这一刀不签管理页或前端目标完成。
