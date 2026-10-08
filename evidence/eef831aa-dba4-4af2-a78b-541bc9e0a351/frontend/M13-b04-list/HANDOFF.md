# M13 交接

G3 `a352` 下次复用同一评审会话时审这次提交。不要新开第四个前端会话。G2 `a353` 以这次提交的 SHA 为准，不要读移动中的工作区。父提交是 `e5658235504c7e32b974ba7d22046e509dce1445`。驱动归档基线是 `83312860b76330284984e7751f16c6b92ab926e0`。拒绝名单复测归档基线是 `b9f1e4fc30f144d1cf7dc61aa7765f58164a9caa`。源文件哈希在 `source-inventory.txt`。

## 可以依赖的接口

- 适配器 `ROUTE_POLICY.selfbuild` 覆盖 `/control/v1/builds`、`/control/v1/reviews`、`/control/v1/classifications` 以及它们的子路径。这些路径先于身份 `/control` 匹配。
- 示例 `apps/web/deploy/b01-adapter.example.json` 的 `selfbuild.upstream` 是 null。此时转发返回 502，正文 `selfbuild_unbound`。listen、pageOrigin、gateway、identity、static、ws 仍是 null。
- `parseBuildList` 只接受已提交的 list 文档。成功结果是 `{ ok: true, count }`。空列表的 count 是 0。多出来的字段失败。
- `callOperation({ operationId: "listBuilds", query: { project_id } })` 在同源传输登记后发送 GET。HTTP 200 且 schema 通过时 `ok` 为 true，`fetched` 为 true，`live`、`journey`、`release` 为 false，`customerIntegration` 为 `not_claimed`。
- selfbuild 的其他 HTTP 200 仍是 `invalid_schema`。403 且 error schema 通过时是 `http_error`，`errorCode` 来自正文。
- `invokeOperation` 对全部 15 个 B04 id 仍返回 `schema_only`。`prepareCall` 仍不发送请求。页面继续使用 `prepareCall`。
- B04 登记状态仍是 `schema_frozen`。合同钉仍是 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。`registerRuntime` 仍拒绝非 null origin。
- 五个身份 operation 仍是 `fixture_verified`。B02 与 B07 仍是 unbound。
- 销毁名单新增 `oo-fe01-m13-edge`、`oo-fe01-m13-proof`、`oo-fe01-m13-idp`、`oo-fe01-m13-pg`、`oo-fe01-m13-b04`。示例配置不含这些主机名。

## 这次证明过的范围

- 回执 `raw/b04-receipt.json`。admin 的空 `listBuilds` 200，缺失 id 的 `getBuild` 403，`business_owner` 的 `listBuilds` 403，两端 logout 200。
- 实现提交 `45644fca81325cf1405a38ceb79435da6e8bb090` 的私有安装。身份提交 `a6f92da8a0bf872f8a30efb116c2c7b14463505b`。身份合同 `089b00d7f70da1bc42f7e201dfa070748c651d5b`。
- 二进制文件不存在，数据库里没有构建行。没有创建构建，没有调用模型端口。
- 服务读取 current 会轮换 CSRF。下一次 logout 要使用这次读取之后的 current。
- m13 主机已经销毁。适配器和 SDK 都会拒绝它们。

## 不要当成已完成

- 不是客户 IdP，不是 live_verified，不是旅程关闭，不是发布，不是 FE-01 完成。
- 空列表不是完成的构建。E56、100k 商店证明、代理闭环和 a373 的发布验收都不在这次回执里。
- 下面 13 个 id 没有真实响应：`createBuild`、`cancelBuild`、`getBuildCards`、`getBuildCandidates`、`listReviewItems`、`getReviewItem`、`previewReviewBatch`、`decideReviewItem`、`listClassifications`、`previewClassifications`、`confirmClassifications`、`compileBuildPack`、`getBuildPack`。
- 页面还不能把 B04 当成已冻结的可调用绑定。G2 继续把 `schema_frozen` 当作当前登记状态。
- 目录 note 里的“生产者二进制未交付”是 a373 交接原句。本次私有运行见回执，不把那一句改写成已验收。
- 不要改 G2 目录。不要用这次 config commit 替换 M9 常量。
- 没有新的浏览器截图。
- 没有修改 G3 oracle、生产者或 `specs/frontend/execution-status.md`。
