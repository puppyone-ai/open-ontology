# M13 发现

1. 第一次 admin logout 失败。selfbuild 的 `gate_read` 会再请求身份 current，服务因此轮换 CSRF。用 B04 读取之前保存的 current 调用 logout，得到失败。第二次在 `listBuilds`、`getBuild` 和 owner 的 403 之后各读一次新 current，再 logout，两次都是 200。`raw/failed-attempts/drive-1.log` 保留第一次断言失败。

2. 套件的 `raw/adapter.log` 被第一次闭环写成空文件。`raw/adapter.exit` 仍是 0。套件当时观察到的适配器输出没有留下正文。拒绝名单复测的 `raw/refuse-adapter.log` 是后来的正文：5 pass，0 fail。`raw/adapter-edge.log` 也是空文件，`raw/edge.exit` 是 0。

3. `launch-suite.sh` 与 `launch-loop.sh` 都写 `raw/archive-base.txt`。文件里留下的 `83312860b76330284984e7751f16c6b92ab926e0` 是驱动归档。套件自己的基线哈希没有保留。从 `5071cc84c6ddb36ac103f44756a3199244bf9968` 到本次父提交，已提交的 `apps/web` 与 `packages/sdk-ts` 没有差异。

4. 套件的 `apps/web` tsc 发生在五个 m13 主机名写入拒绝名单之前。拒绝名单复测跑了 SDK tsc 和更新后的 `sdk-client.test.ts` Vitest，退出码是 0，没有再跑一份 `apps/web` tsc。新增断言是拒绝 `https://oo-fe01-m13-edge:8443`。

5. 目录里的 B04 note 仍引用 a373 交接里的“生产者二进制未交付”。本次回执已经在私有空库上运行实现提交 `45644fca81325cf1405a38ceb79435da6e8bb090` 的 list 与 get。E56、100k、代理闭环和发布仍未验收。登记状态保持 `schema_frozen`，以免 `slices-oracle.mjs` 要求的集合被改写。

6. 页面源码没有 `fetch(`。G2 要等的页面级冻结还没有发生。`callOperation` 可以发送 `listBuilds`。`prepareCall` 与 `invokeOperation` 仍不发送。13 个 B04 operation 没有真实响应。

7. 这次 config commit `95bcd295cc74107110a580b50e4c2e1e8eb8b3d7cebfbf564d7b761f90f5cd42` 只属于 m13 夹具。它不替换 M9 常量，也不替换 M12 回执里的值。客户集成保持 `not_claimed`。

8. B02 与 B07 仍是 unbound。`POST /v1/sql` 的 501 仍不是 B09。没有修改 G2 目录、G3 oracle、生产者或 `specs/frontend/execution-status.md`。
