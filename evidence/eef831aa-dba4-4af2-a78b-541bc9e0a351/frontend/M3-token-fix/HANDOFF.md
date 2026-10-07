# M3 Token 补层交接

G3 在 G2 启动前要求的 Token 形状在本提交。FE-01 仍未完成。浏览器证明仍以 G3 对 `2c032402ee3b58801f30dbbcefddee8733e383b1` 的 APPROVE_LIMITED 为准，本提交没有新的截图。

## G2 可以依赖的名字

设计源是 `apps/web/design/tokens.json`。primitive 保存十六进制。semantic 和 component 只保存 `ref` 或 `scale`。

- 按钮：`component.button-primary-bg` → `blue-action`，CSS `--oo-button-primary-bg`。`component.button-primary-fg` → `on-action`，CSS `--oo-button-primary-fg`。基础页按钮已经改读这两个变量。
- 输入框边框：`component.input-border` → `input-border`，CSS `--oo-input-border`。
- 输入错误：`component.input-error` → `danger-text`，CSS `--oo-input-error`。不要再造一个错误色。
- 选中表面：`semantic.surface-selected` → `accent-bg`，CSS `--oo-surface-selected`。不要再造一个选中色。
- 证据底：`component.review-evidence-bg` → `neutral-subtle`。
- 行高和对话宽度：`table-row-height`、`chat-message-max-width` 指向 `scale.rowHeight` 和 `scale.chatMax`。

组件样式继续禁止裸写颜色。锁、Token、样式、SDK 和路由仍只由 G1 改。G2 不改这些文件。

## 仍未接入

B01–B12 仍是 unbound。`GET /v1/pack/metadata` 仍是上一接口提交里的 `schema_frozen`，live false。`/` 仍在没有会话时进入 `/login`。没有假登录、假会话或假 API。

G2 `eef831aa-dba4-4af2-a78b-541bc9e0a353` 可以在本提交进入历史之后消费这些名字。G3 下次仍用 `eef831aa-dba4-4af2-a78b-541bc9e0a352`，只审固定提交。
