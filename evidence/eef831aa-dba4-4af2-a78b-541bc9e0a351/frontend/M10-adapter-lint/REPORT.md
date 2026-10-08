# M10 适配器 lint 与显式部署配置

测试归档是 `3b35c82795a0ac9a4d9c0838d93e99703a938266`。写报告时 HEAD 已到 `7169d2a839fffb898f14f2ed790e99180a5542bb`。这两次提交之间 `apps/web` 和 `packages/sdk-ts` 没有差异。提交脚本若发现父提交不再是 `7169d2a8`，会停住。这不是 FE-01 完成，也不是客户上线。live、journey、release 都是 false。

## lint

G2 在固定源 `9c75d686bf21d0859b833b279b36dc9cc78e9912` 上跑 eslint，`apps/web/deploy/b01-adapter.mjs` 与 `b01-adapter.test.mjs` 有 4 个 `no-undef`：`URL` 两处、`Buffer` 两处，退出码 1。`.mjs` 配置原先只声明了 `console` 和 `process`。

这次在同一份配置里补上 Node 实际提供的 `Buffer`、`URL`，以及全量 lint 接着报出的 `structuredClone`。`structuredClone` 出现在 `apps/web/tests/acceptance/slices-oracle.mjs`。没有改那份 oracle，也没有关闭 `no-undef`。

Linux Node v24.21.0 上 `eslint --config config/eslint.config.mjs .` 的退出码是 0。`raw/lint.log` 为空。`raw/lint.exit` 是 0。

## 部署配置

`b01-adapter.example.json` 的 `listen.host`、`listen.port`、TLS 和全部上游都是 null。`readAdapterConfig` 在 `listen.host` 不是非空字符串时抛出 `listen.host is required`，不再写成 `127.0.0.1`。示例本身仍因缺少身份上游而拒绝。能监听的配置只来自部署者提供的 `OO_B01_ADAPTER_CONFIG`。已销毁的证明主机仍被拒绝。

`node --test deploy/b01-adapter.test.mjs` 是 5 通过。`raw/adapter-test.exit` 是 0。

## 组合测试

归档基线是 `3b35c82795a0ac9a4d9c0838d93e99703a938266`，再覆盖当时尚未提交的 eslint 配置和适配器文件。三份 `tsc` 退出码都是 0，日志为空。

Vitest 是 31 个文件、67 个测试通过，用时 27.13 秒。`raw/vitest.exit` 是 0。其中 `production-route.test.tsx` 3 个测试、`modules.test.tsx` 3 个测试都通过。日志里没有跳过。锁文件前后都是 `73cb84555227eff9c862a40cbd1ffb1d35fd3a5c11cfa757b0ad18436d60c80e`。

M8 在 `96489b1cffeeaef3bd8ccfeb4d1d7b3449c757e9` 上的 63 个测试是当时的结果。这次是当前树上的全量，不是把那次记录再跑一遍来改写旧报告。

没有重跑字体，也没有重跑 M9 的浏览器。五个角色的浏览器证明仍是 `ac8737ab2e9ef3d6de41ce2527f487222ce52373`。

## B04

合同仍是 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。实现提交 `2d7c2a5cd9c3193a7fd3d3124642da194ef8683f` 仍是祖先。a373 的 `HANDOFF.md` 仍写明二进制未交付、`a6f92da8` 未被独立验收、原 E56 未验收。这一轮没有接 B04 的 HTTP，也没有伪造响应。状态保持 `schema_frozen`。运行时 origin 保持 null。五个身份 operation 仍是 `fixture_verified`，服务提交仍是 `a6f92da8a0bf872f8a30efb116c2c7b14463505b`。

## 资源

容器 `oo-fe01-node-m10` 已删除。内存 4294967296，CPU 1，`CapDrop ALL`，`Privileged=false`。cgroup `memory.max=4294967296`，`cpu.max=100000 100000`。网络是 Docker 默认 bridge，不是 host。没有发布端口。启动时 `node_running=0`、`own_node=0`。私有归档已删除。没有密钥。
