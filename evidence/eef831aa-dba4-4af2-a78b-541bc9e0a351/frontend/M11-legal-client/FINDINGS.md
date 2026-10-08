# M11 发现

1. B02 与 B07 没有已提交的 OpenAPI。未跟踪的 `tools/source-control-api`、`tools/secret-control-api`、`tools/release-control-api` 不能当作冻结。SDK 不收录这些树里的 operationId。G2 `e86a4900` 交接已经要求源和发布先固定提交。
2. B04 合同仍是 `7bb2da3cc996a5f0ed6e41e27734c3ea390d44e9`。a373 交接仍写 E56 未验收、生产者二进制未交付、身份产品 `a6f92da8` 未验收。实现提交 `2d7c2a5cd9c3193a7fd3d3124642da194ef8683f` 是祖先，但不是可调用交付。ready 保持 false。
3. 已提交 OpenAPI 的 operation 顺序把 `previewReviewBatch` 放在 `getReviewItem` 前。`CONTRACT.md` 和 `SELFBUILD_HTTP` 的顺序相反。两边都是这 15 个 id。客户端按 id 字符串绑定，没有改登记顺序，以免动到已有的 `OPERATIONS` 断言。
4. 第一次 Vitest 退出码 1，就是第 3 条的顺序断言。改成集合比较后，第二次 `sdk-client.test.ts` 4 个测试退出码 0。失败日志留在 `attempt-1/vitest.log`。
5. 工作区里 `tools/selfbuild-control-api/src` 有未提交修改。这次归档没有带上它们，也没有把它们提交。
6. `prepareCall` 可以标出 CSRF 已绑定，但返回值不含 token，也不发请求。变更在服务就绪前仍是 `schema_only` 或 `not_connected`。
7. 全旅程、客户集成、live、release 都仍是 false。构建、审核、分类、发布、用户问答没有真实调用。
