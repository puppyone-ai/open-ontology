# M13 构建列表与详情

角色：G2，UUID `eef831aa-dba4-4af2-a78b-541bc9e0a353`。这一刀消费 G1 M13 固定源 `61c2d356124f8631fb5fbc490de67c87c00d2964`（`frontend: accept an empty listBuilds from the selfbuild service`）。构建页发送 `listBuilds`。详情页只在符合 `b_` 加 32 位十六进制的地址上发送 `getBuild`。空列表不是完成的构建。其余 13 个 B04 operation 不发送。这一刀不签管理页、B04 或前端目标完成。

## 改动

G1 报告里的私有空库回执：admin 的 `listBuilds?project_id=alpha` 是 200，count 是 0；缺失 id 的 `getBuild` 是 403 `forbidden`。`getBuild` 的 HTTP 200 仍是 `invalid_schema`。登记仍是 `schema_frozen`。`prepareCall` 与 `invokeOperation` 仍不发送。页面没有 `fetch(`。产品页面不传入 `fetchImpl`。

六个自有文件：

- `apps/web/src/features/builds/build-list.ts` sha256 `652de66d821b91d93e7d3ede7b68d54aaa9f9336b69f1bc74412f364878a16b0`
- `apps/web/src/features/builds/pages.tsx` sha256 `449f48198801b17e25be429d747721aaf3078c968de586ce4e155189dac02f1f`
- `apps/web/src/features/builds/build-read.test.tsx` sha256 `ad04187a75cf2fcc77415501eb47b20dd7b64807531e6b9ade22446c5d8a7f90`
- `apps/web/src/features/builds/APIBINDING.md` sha256 `1f313f99e068de3e83ded423785a97b7a2bc11e8e709d55cb29398882fd1774f`
- `apps/web/src/features/reviews/APIBINDING.md` sha256 `a1d57aab8a1bf28534b82f1fc20bccd56076b4b05b2a49882206fe4a54a19ea0`
- `apps/web/src/features/classifications/APIBINDING.md` sha256 `6ddf3426466b0b28b951f7fc3dfddf7bd36c4b338957083f0556d079cb7e148b`

审核页和分类页的组件没有改，也不调用 `callOperation`。运行构建和重试保持禁用。M4 的详情页仍显示「没有运行记录」。

## 已证明

检查用的是 `61c2d356124f8631fb5fbc490de67c87c00d2964` 的 `apps/web`、`packages/sdk-ts`、锁、`tools/control-identity` 和 `tools/selfbuild-control-api`，再覆盖上面六个文件。`router.tsx`、`admin-gate.tsx`、`identity-session.ts`、`identity-context.tsx`、`bindings.ts`、`client.ts`、`index.ts`、eslint 配置、`blocks.tsx`、`nav.ts`、审核页、分类页和 `pnpm-lock.yaml` 与固定提交逐字节一致，`frozen_cmp=ok`。锁是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。

容器 `oo-a353-node-m13`。镜像 `docker.io/library/node@sha256:481d53bf381928c9e3bb5cedf90687d81a78986cf4d7fe5a5fdfc8de1c5fe413`。Node v24.21.0，pnpm 12.10.1，aarch64，容器内 uid 0。4GiB 内存、4GiB swap、1 CPU、pids 2048、`--cap-drop ALL`、`privileged=false`、网络 `none`。没有安装。node_modules 从这一 UUID 的 M0 私有副本克隆。

| 阶段 | 退出码 |
|---|---|
| tsc app | 0 |
| tsc node | 0 |
| tsc sdk | 0 |
| eslint | 0。日志为空 |
| vitest | 0。35 个文件，84 个测试。日志 `Start at 09:35:15`，耗时 36.35s |
| docker | 0 |

`build-read.test.tsx` 4 个测试，386ms。第一次归档即完成。容器已删除。

Rust 和 Clippy 没有跑。生产包这次没有重新构建。浏览器没有打开。页面测试里的响应用来核对客户端解析，不是 a373 的新服务，也不是那 13 个 operation 的完成。

## 未证明

`createBuild`、`cancelBuild`、`getBuildCards`、`getBuildCandidates`、`listReviewItems`、`getReviewItem`、`previewReviewBatch`、`decideReviewItem`、`listClassifications`、`previewClassifications`、`confirmClassifications`、`compileBuildPack`、`getBuildPack` 没有发送。审核和分类仍未读取。空列表不是完成的构建。`getBuild` 没有成功文档。`live`、`journey`、`release` 仍是 false。运行时 origin 仍是 null。E56、100k 和发布仍未验收。

证明时工作区是 `13ad15b4bc66979b96a2604c20e2ca4b20dd92f7`，`m13_ancestor=yes`。这次提交的父提交是 `e2bd526e96175efb52fa1181843fbe991ae7992b`。这一刀不签管理页或前端目标完成。
