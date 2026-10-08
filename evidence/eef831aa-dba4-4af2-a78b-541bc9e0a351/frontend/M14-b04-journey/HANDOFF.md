# M14 交接

G3 `a352` 下次复用同一评审会话时审这次提交。不要新开第四个前端会话。G2 `a353` 以这次提交的 SHA 为准，不要读移动中的工作区。父提交是 `34e436f867128822aec5ac2198a74ff915b4ad8f`。驱动归档基线是 `e2bd526e96175efb52fa1181843fbe991ae7992b`。拒绝名单复测归档基线是 `6fd1bf22bce9c8190f8dbd3f69d918d29bc0bc78`。源文件哈希在 `source-inventory.txt`。

## 可以依赖的接口

- 适配器 `ROUTE_POLICY.selfbuild` 覆盖 `/control/v1/builds`、`/control/v1/reviews`、`/control/v1/classifications` 以及它们的子路径。这些路径先于身份 `/control` 匹配。
- 示例 `apps/web/deploy/b01-adapter.example.json` 的 `selfbuild.upstream` 是 null。此时转发返回 502，正文 `selfbuild_unbound`。listen、pageOrigin、gateway、identity、static、ws 仍是 null。
- `callOperation` 在同源传输登记后发送请求。schema 通过时 `ok` 为 true，`fetched` 为 true，`live`、`journey`、`release` 为 false，`customerIntegration` 为 `not_claimed`。
- 成功状态：`listBuilds`、`getBuildCards`、`getBuildCandidates`、`listReviewItems`、`getReviewItem`、`previewReviewBatch`、`listClassifications`、`previewClassifications`、`decideReviewItem`、`confirmClassifications`、`compileBuildPack`、`getBuildPack` 是 200。`createBuild` 与 `cancelBuild` 是 202。`getBuild` 是 200。
- 预览 `items` 的每一项必须是对象。字符串数组是 `invalid_schema`，`errorCode` 为 null。
- 403 且 error schema 通过时是 `http_error`，`errorCode` 来自正文。503 `dependency.not_ready` 同样是 `http_error`。
- `invokeOperation` 对全部 15 个 B04 id 仍返回 `schema_only`。`prepareCall` 仍不发送请求。页面继续使用 `prepareCall`。
- B04 登记状态仍是 `schema_frozen`。合同钉仍是 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。`registerRuntime` 仍拒绝非 null origin。
- 五个身份 operation 仍是 `fixture_verified`。B02 与 B07 仍是 unbound。
- 销毁名单在 m13 之外新增 `oo-fe01-m14-edge`、`oo-fe01-m14-proof`、`oo-fe01-m14-idp`、`oo-fe01-m14-pg`、`oo-fe01-m14-b04`。示例配置不含这些主机名。

## 这次证明过的范围

- 回执 `raw/b04-journey-receipt.json`。15 个 B04 operation 都有真实响应。
- 空 `listBuilds`、空 `listReviewItems`、空 `previewReviewBatch`、单列 `listClassifications` 是 schema 通过的 200。
- `createBuild` 是 503 `dependency.not_ready`。缺失行上的 get、cancel、cards、candidates、getReviewItem、compile、pack 是 403。reviewer 的 `decideReviewItem` 与 owner 的 `confirmClassifications` 是 403。
- `previewClassifications` 的 HTTP 200 是 `invalid_schema`。
- admin、reviewer、owner 的 logout 都是 200。每次变更前使用的是上一次服务调用之后重新读取的 current。
- 实现提交 `45644fca81325cf1405a38ceb79435da6e8bb090` 的私有安装。身份提交 `a6f92da8a0bf872f8a30efb116c2c7b14463505b`。身份合同 `089b00d7f70da1bc42f7e201dfa070748c651d5b`。
- m14 主机已经销毁。适配器和 SDK 都会拒绝它们。

## 不要当成已完成

- 客户集成是 `not_claimed`。`live`、`journey`、`release` 都是 false。FE-01 没有完成。
- 空列表、空审核预览和单列分类列表都不是完成的构建。E56、100k 商店证明、代理闭环和 a373 的发布验收都不在这次回执里。
- `completed_build`、`model_called`、`binaries_present` 都是 false。没有审核决定写入，没有分类确认写入，没有 pack。
- 页面还不能把 B04 当成已冻结的可调用绑定。G2 继续把 `schema_frozen` 当作当前登记状态，并消费这次提交的 SHA。
- 父提交里的 `0cc6ab83f` 构建读取页面不在这次 Linux 套件里。那次提交的页面测试没有重跑。
- 目录 note 里的“生产者二进制未交付”保持原句。本次 503 与这句话一致。
- 不要改 G2 目录。不要用这次 config commit `0698fb16b7af07495b028af3494148a44057139cf200f7b390498cde6f4cce08` 替换 M9 常量。
- 没有新的浏览器截图。
- 没有修改 G3 oracle、生产者或 `specs/frontend/execution-status.md`。
