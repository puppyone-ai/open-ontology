# M5 管理模块守卫与冻结 schema

角色：G1，UUID `eef831aa-dba4-4af2-a78b-541bc9e0a351`。FE-01 未完成。live、完整旅程、发布都是 false。本报告不自签验收。

证明运行时的工作区基线是 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。M4 `082af80b31d7d932be7c0acbe21f22aa122cc24c`、身份合同 `089b00d7f70da1bc42f7e201dfa070748c651d5b` 和 G2 固定快照 `d416874eee65e5f30ad9858cef4429f608066c7a` 都是祖先。提交前 HEAD 已前进到 `9d33bfc065901db78e8f736e3a840b57630a31e4`。`git diff --name-only 7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9..9d33bfc065901db78e8f736e3a840b57630a31e4` 没有碰到本里程碑的源文件。后到的提交包括 G2 的 `e090131ed`、身份实现 `35bd6b815` 和身份测试发行者证据 `00135ecd5`。本里程碑没有调用这些服务，也没有把绑定改成 live。审阅目标是包含本文件的那一次 owned-path 提交。若提交时 HEAD 不再是 `9d33bfc065901db78e8f736e3a840b57630a31e4`，提交脚本会停下来。

## 改了什么

1. `ADMIN_MODULES` 等于 G2 固定快照里的 `ADMIN_FEATURE_MODULES`，共 41 条，A03 到 A43。生产路由用 `GuardedAdminModule`。`getIdentityCurrent` 不是 `live_verified` 时，访问这些路径会替换到 `/admin/login`，并且不调用 `module.load()`。`/admin/login` 和 `/admin/bootstrap` 仍是公开的未接入页。没有 cookie、localStorage 身份、密码或假主体。
2. Storybook 的 stories 增加了 `src/components` 和 `src/features`。没有改 G2 的 story 文件。
3. `SecretInput` 增加可选的 `connection`：`unbound`、`forbidden`、`ready`。省略时仍按原来的 `canWrite` 解释。`unbound` 显示「此能力尚未接入。依赖 B07。」，字段停用，即使 `canWrite` 为 true 也不调用 `onReplace`。
4. `ReviewDecisionForm` 在 `disabled` 或 `readOnly` 时，理由框同时 `disabled` 和 `readOnly`。G2 的 `ReviewDesk`、`SOURCE_STEPS` 和导航没有改。
5. SDK 为控制身份的五个 OpenAPI operation，以及 B04 合同 `7bb2da3cc` 的十五个 HTTP operation，增加 `schema_frozen` 记录。`invokeOperation` 返回 `schema_only`。占位的 `B01.*` 和 `B04.*` 仍是 unbound。没有 fetch，没有 CSRF，没有把钉住的身份 JSON 当成成功响应。
6. `apps/web/deploy/README.md` 和 `runtimeConfig` 写明两个 origin 都是 null，Storybook 不进生产包。不拥有 Compose、Caddy 或 CI。
7. 原来要求「设计基线」的 19 个视觉测试改为登录页的视口、主题、密度和键盘检查，截图使用 `login-` 前缀。`foundation.behavior.spec.ts` 的 3 个旧夹具检查改为生产路由：未登录无密码、字体留在本源且不请求完整字体、禁用的发送按钮不离开会话、390 宽的菜单按钮不低于 44px。旧的 FE-01 证据目录没有覆盖。

## 已证明

镜像 `docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413`。Node v24.21.0。两只容器都是 4GiB、1 CPU、`--cap-drop ALL`、私有网桥。第一只的 cgroup `memory.max` 是 4294967296，`cpu.max` 是 `100000 100000`。第二只的 HostConfig 是 Memory=4294967296、NanoCpus=1000000000、CapDrop ALL。锁没有变：`73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。

外壳计时不是证明。证明是容器日志、PNG 头和字体请求清单。

| 阶段 | 容器 | 结果 |
|---|---|---|
| 字体生成 | `oo-fe01-node-m5` | `raw/font-build.log`，`subsetPoints=506`，完整字体 7762252 字节 |
| Vitest，去掉两份会因空数组失败的 G2 测试 | 同上 | `raw/vitest.log`，27 个文件，54 个测试通过 |
| 类型检查、改动文件 lint、Vite 构建、Storybook | 同上 | `raw/exits.txt` 均为 0。Storybook 构建完成 |
| apt 与 ldd | 同上 | 退出 0。apt 末尾删除缓存的 permission denied 不改变退出码 |
| Playwright 视觉 | 同上 | `raw/playwright-visual.log`，21 个测试通过。22 张 PNG，尺寸与视口一致，每张大于 2000 字节 |
| Playwright 无障碍 | 同上 | `raw/playwright-a11y.log`，2 个测试通过。登录标题对比浅色 15.006、深色 14.97。严重和关键违规为 0 |
| 理由框、SDK、运行配置 | `oo-fe01-node-m5b` | `raw/focused-vitest.log`，3 个文件，17 个测试通过 |
| 最终生产构建 | 同上 | `raw/focused-bundle.log`。JS `index-NShXIz5d.js` 344710 字节，gzip 108337。CSS 28241 字节，gzip 6278。HTML 不包含 `pages-` 预加载 |
| 生产行为 | 同上 | `raw/focused-playwright.log`，3 个测试通过 |

字体请求 `raw/font-requests.json` 有 39 条，状态只有 200 和 304，路径只有 Geist 和三档 Noto 子集。没有完整字体，没有外部字体主机。

子集 sha256：

| 文件 | 字节 | sha256 |
|---|---|---|
| `NotoSansSC-300-subset.woff2` | 70292 | `a61419510dd6c3b7698a6f9f32c15d4ab44291500103e61befe5eafa22b487f3` |
| `NotoSansSC-400-subset.woff2` | 71612 | `67de1ac1e892e832f070d5544da0bb225f9ec711d8347ba3b8b46f0f92fd4640` |
| `NotoSansSC-500-subset.woff2` | 72492 | `e6b27f2423b13bccbb834b3c95f5b5a140e1897b3e49660c9bc657c907742615` |
| `NotoSansSC-full.woff2` | 7762252 | `bd3f8367b353276b7c8e2a3385c9a48bdaf9be1a010413a6d70cb94eaf66d40b` |

## 没有证明

没有调用 a370 或任何控制身份、自建服务。没有真实登录、会话、CSRF 或审核提交。B01 到 B12 的占位 operation 仍是 unbound。`schema_frozen` 不是 live。FE-01、旅程和发布都未完成。
