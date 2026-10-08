# M14 发现

1. 保留的 `raw/launch-suite.exit`、`raw/lint.exit` 和 `raw/lint.log` 是同一次成功运行。lint 是 0 个 error、2 个 warning，套件退出码是 0。`raw/failed-attempts/` 是空目录。同一次里程碑里更早的 eslint `no-useless-assignment` 日志被第二次套件覆盖，包里没有那份原文。提交里的 `b04-journey-drive.test.ts` 把 `rotate()` 放在每个要使用返回 current 的 POST 之前。这次驱动的三次 logout 都是 200，没有第二次驱动。

2. `raw/adapter.log` 仍是套件的适配器 `node:test` 正文：5 pass，0 fail。`raw/adapter-edge.log` 与 `raw/node-loop.log` 是空文件。`raw/edge.exit` 是 0。驱动标准输出在 `raw/drive.log`，回执在 `raw/b04-journey-receipt.json`。M13 里套件 `adapter.log` 被闭环写空的情况没有重复。

3. `launch-suite.sh` 与 `launch-loop.sh` 都写 `raw/archive-base.txt`。文件里留下的 `e2bd526e96175efb52fa1181843fbe991ae7992b` 是驱动归档。拒绝名单复测另写 `raw/refuse-archive-base.txt`，值是 `6fd1bf22bce9c8190f8dbd3f69d918d29bc0bc78`。归档基线之后、本报告父提交之前，已提交的 `apps/web` 增加了 `0cc6ab83f` 的构建读取页面。这段里 `packages/sdk-ts` 没有已提交差异。套件和驱动跑在归档加上当时未提交的 G1 覆盖上，没有重跑 `0cc6ab83f` 的页面测试。

4. 套件的 `apps/web` tsc 发生在五个 m14 主机名写入拒绝名单之前。拒绝名单复测跑了 SDK tsc、eslint、适配器测试和更新后的 `sdk-client.test.ts`，退出码是 0，没有再跑一份 `apps/web` tsc，也没有重跑驱动。新增断言是拒绝 `https://oo-fe01-m14-edge:8443`。Vitest 仍是 9 个测试，这条断言在已有用例里。

5. install 与 serve 的第一行是 `dependency` `ready`。那是服务进程启动。五个工具二进制文件不存在。`createBuild` 返回 503 `dependency.not_ready`，回执 `binaries_present` 是 false。没有用替身文件通过 `binaries_ready`，没有调用模型端口。

6. `previewClassifications` 的 HTTP 200 被记为 `invalid_schema`。已提交 OpenAPI 要求预览 items 是对象。服务返回的是字符串数组。客户端没有放宽这个检查，生产者也没有被修改。空的 `previewReviewBatch` 因 items 为空数组而通过。

7. selfbuild 的读取和变更都会再请求身份 current，CSRF 因此轮换。这次驱动在每个使用 current 的 POST 之前读取一次新的 current。三次 logout 都是 200。回执里没有 csrf、cookie 或 bootstrap token 正文。`consumeBootstrapToken` 只作为 operationId 出现。

8. 目录里的 B04 note 仍引用 a373 交接里的“生产者二进制未交付”。本次 `createBuild` 的 503 与这句话一致。E56、100k、代理闭环和发布仍未验收。登记状态保持 `schema_frozen`，以免 `slices-oracle.mjs` 要求的集合被改写。页面源码没有 `fetch(`。`prepareCall` 与 `invokeOperation` 仍不发送请求。

9. 这次 config commit `0698fb16b7af07495b028af3494148a44057139cf200f7b390498cde6f4cce08` 只属于 m14 夹具。它不替换 M9 常量，也不替换 M12 或 M13 回执里的值。客户集成保持 `not_claimed`。`live`、`journey`、`release` 都是 false。

10. B02 与 B07 仍是 unbound。`POST /v1/sql` 的 501 仍不是 B09。没有修改 G2 目录、G3 oracle、生产者或 `specs/frontend/execution-status.md`。`apps/web/.storybook/preview.tsx` 的未提交改动不属于这次提交。
