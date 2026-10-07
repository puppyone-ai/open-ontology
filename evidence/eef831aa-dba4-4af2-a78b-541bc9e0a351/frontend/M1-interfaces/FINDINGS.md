# M1 公共接口发现

## 测试失败（已保留，随后修过）

`oo-fe01-node-m1` 的 Vitest 退出码 1：22 个测试里 2 个失败，都在 `shared.behavior.test.tsx`。原始日志 `raw/test-attempt-1.log`。

1. 小数句子被拆成多个文本节点，`getByText("条形长度是近似。表格里的数字是原文字。")` 匹配不到。组件随后改成一个模板字符串：`单位 ${unit}。条形长度是近似。表格里的数字是原文字。`
2. 测试用原生 `dispatchEvent` 发送 composition 和 Enter。React 19 的合成监听没有把 `composing` 标成 true，`onSend` 被调用了一次。测试改用 `fireEvent.compositionStart` / `keyDown` / `compositionEnd`。
3. 同一文件里 DOM 在用例之间累积，因为 Vitest 没有打开 globals，Testing Library 不会自动 `cleanup`。`test-setup.ts` 增加 `afterEach(cleanup)`。

`oo-fe01-node-m1b` 只重跑测试，退出码 1。IME 用例已通过。小数用例仍用子串做精确 `getByText`，节点全文是 `单位 1。条形长度是近似。表格里的数字是原文字。`，精确匹配失败。日志 `raw/test-attempt-2.log`。断言改成这句全文。组件在这次之后没有再改。

`oo-fe01-node-m1c` 在最终树上重跑类型、lint、测试和构建，测试 22 通过，容器退出码 0。前两次失败日志没有覆盖。

## 浏览器

本里程碑没有浏览器证明。基础里程碑里，在 `--cap-drop ALL` 的非特权容器中用 apt 装浏览器库失败。那次缺口留给 G3。本会话没有重试 apt，也没有把短启动当成截图、axe 或键盘证明。

## 绑定边界

产品组的 OpenAPI 不存在。SDK 不从 `contracts/schema/openapi-v1.json` 生成客户端。

`GET /health`、`GET /ready`、`GET /v1/session/current`、探针 `GET /slow` 和 `GET /hold` 只登记为进程内存在，不是产品绑定。`POST /v1/sql` 固定 501 `SQL_UNSUPPORTED`，不是 B09 查询。

解析器拒绝大于 `9007199254740991` 的 JSON 数字，原因是 `unsafe_integer`，不把 u64 四舍五入。未知键是 `unrecognizedKeys`。未知错误码是 `incompatible_enum`。

## 工作区里没有纳入本提交的内容

`apps/web/tests/` 是未跟踪的外来文件，包含 acceptance oracle 和 foundation browser spec。没有 `git add`。

`specs/**` 与 `tools/review-ledger/**` 在本会话开始时已经有别人的未提交修改。没有暂存，没有改写。`specs/frontend/execution-status.md` 仍由 Codex 单独写入，本会话没有追加状态。

## 资源

三只容器在退出后仍留在 `docker ps -a`，因为启动时没有 `--rm`。检查记录在 `raw/containers.txt`。清理只按这三个名字 `docker rm`，不 prune，不碰其他容器。结果在 `raw/cleanup.txt`。

全局 Node 构建槽在本里程碑占用过 1。浏览器槽没有占用。G3 的浏览器槽没有被本会话抢用。
