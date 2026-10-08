# M7 当前身份闭环报告

角色：G2，UUID `eef831aa-dba4-4af2-a78b-541bc9e0a353`。这一刀让管理壳读取已解析的当前会话，并让「退出」调用身份客户端。不签页面验收，不签 live、旅程或发行。

## 改动

固定源 `9c75d686bf21d0859b833b279b36dc9cc78e9912`。身份服务提交 `a6f92da8a0bf872f8a30efb116c2c7b14463505b`。自己的守卫测试 `e74ae63e8cf6991bb34f613ab88122bb21d1944d` 仍在这份归档里。`production-route.test.tsx` 没有改。

- `apps/web/src/features/admin-overview/admin-frame.tsx` sha256 `981e56509abc06a4ee2a3583ff90d990b1df50c151c86023339190ee0d2dbe36`。已解析会话时，壳显示主体、项目和角色，并给出「退出」。退出调用 `logoutManagementSession`。结果为 `revoked` 时刷新身份。其他结果留下「这一步没有完成。」，不刷新。没有会话时不显示退出，不发请求。`modules` 仍是空数组，导航仍来自 `ADMIN_NAV`。
- `apps/web/src/features/permissions/pages.tsx` sha256 `1284d95a1508dc204d6dfb5da168ee05ab40a40fc77cfa1d08fdd4c24063eeb3`。角色、身份绑定、权限预览显示主体、初始化状态，以及每个项目的角色和能力。应用、保存、预览保持禁用。
- `apps/web/src/features/permissions/current-session.test.tsx` sha256 `53b75ce3796354cec7939a58e83a23777b7d491375e952b6370d72a61b9028ee`。三条例：没有提供者时不发明角色、不调用 `fetch`；`project.admin` 会话显示能力，退出为 `POST /control/v1/auth/logout` 且带 `x-oo-csrf`，随后 401 回到「管理登录」；logout 403 时角色还在。
- `apps/web/src/features/permissions/APIBINDING.md` sha256 `22fae03a81a244ee692b735b79cb9e15b931092380caaf6bfdc42cd16b07ab5a`。五个占位仍是 unbound。当前和退出走同源 `/control/v1/`。引导令牌仍由 `/admin/bootstrap` 的 `consumeBootstrapToken` 提交。

M4 三处仍在这份归档里：连接步骤没有 secret 字段，标题是「连接」，字段是「数据源名称」；`ADMIN_NAV` 没有 `:id`；`ReviewDesk` 是「没有候选」加空证据，不引用 `ReviewDecisionForm`。

## 已证明

检查用的是上述固定提交的 `apps/web`、`packages/sdk-ts`、锁、`tools/control-identity` 和 `tools/selfbuild-control-api`，再覆盖上面四个文件。`router.tsx`、`admin-gate.tsx`、`identity-context.tsx`、`identity-session.ts`、`bindings.ts`、字体清单、`fonts.css`、`tokens.css`、`wizard.tsx`、`review-decision-form.tsx`、`app-shell.tsx`、`blocks.tsx`、`nav.ts`、`b01-adapter.mjs` 与固定提交逐字节一致，哈希在 `raw/frozen-sha256.txt`。锁是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。

容器 `oo-a353-node-m7`。镜像 `docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413`。Node v24.21.0，pnpm 12.10.1，aarch64。4GiB 内存、4GiB swap、1 CPU、pids 2048、`--cap-drop ALL`、`privileged=false`、网络 `none`。没有安装。node_modules 从这一 UUID 的 M0 私有副本克隆。

| 阶段 | 退出码 |
|---|---|
| tsc app | 0 |
| tsc node | 0 |
| tsc sdk | 0 |
| eslint | 1 |
| vitest | 0。31 个文件，66 个测试。开始 04:26:08，耗时 44.04s |
| docker | 1。内层脚本把最差阶段码作为容器退出码 |

通过的包括 `current-session.test.tsx` 3 个测试（373ms）、`production-route.test.tsx` 3 个（827ms）、`modules.test.tsx` 3 个、`permissions/feature.test.tsx` 1 个、`font-coverage.test.ts` 3 个、`sdk-binding.test.ts` 4 个。

eslint 的 4 个错误都在 `apps/web/deploy/b01-adapter.mjs:132` 的 `URL`，以及 `b01-adapter.test.mjs:11`、`:12` 的 `URL` 和 `:150` 的 `Buffer`。这两个文件与 `9c75d686b` 逐字节一致。自有文件没有出现在 `lint.log`。G2 没有改部署适配器，也没有改 eslint 配置。

Rust 和 Clippy 没有跑。这一刀没有 Rust。生产包这次没有重新构建。浏览器没有打开。测试里的 `fetch` 是脚本桩，用来核对真实客户端的路径、方法和 CSRF 头。

## 未证明

运行时 origin 仍是 null。`live`、`journey`、`release` 仍是 false。`invokeOperation` 对五个身份 id 返回 `not_connected`，不发送请求。容器网络是 `none`，没有对真实发行方做登录、引导消费或退出。客户集成 `not_claimed`。销毁过的测试发行方主机名没有写进运行配置。

`B01.projectRoleBinding` 没有写入接口。应用、保存、预览保持禁用。当前会话的展示不签成角色绑定完成。

B04 仍等 source383 和 build373 的固定合同。`createBuild` 是 `schema_frozen`，占位 `B04.builder` 是 unbound。构建、审核分类和 compiled pack 没有串起来。

检查时工作区父提交是 `96489b1cffeeaef3bd8ccfeb4d1d7b3449c757e9`，说明是 `frontend: keep API, OIDC, and WebSocket off the SPA shell`。它改了 G1 的 deploy 适配器，不在这次容器归档里。这次提交只加入上面四个源文件和本目录。

这一刀不签管理页或前端目标完成。
