# M2 用户路由交接

本提交在 `e61c0fd98b6c8b09c9c8e2db3a441aafaf952909` 的公共接口之上增加用户路由。FE-01 未完成。浏览器证明仍 withheld。G2 `eef831aa-dba4-4af2-a78b-541bc9e0a353` 仍不改锁、Token、样式、SDK、路由和 G1 功能目录。G3 不从本会话派生。

## 当前生产路由

- `/` 重定向到 `/login`。客户端没有会话存储。
- `/conversations/:conversationId` 渲染未接入对话。发送、停止本轮、结束会话都是禁用状态。页面不回显路径 id。
- `/conversations/:conversationId/files` 渲染未接入文件页。没有下载。
- `/knowledge` 渲染未接入知识页。不列出目录或对象名。
- `/account` 渲染未接入账户页。不回读凭证，没有退出按钮。页顶偏好控件沿用 M1 的三个非身份键。
- `apps/web/src/features/user/chat-home-page.tsx` 的 `ChatHomePage` 是 U02 未接入表面。它还没有挂到 `/`。有真实会话之前不要把它当成已登录对话。

用户壳导航只含对话、知识、账户。`ADMIN_MODULES` 仍是空数组。管理对象名不出现在这些页面。

## 绑定

没有新操作。`invokeOperation` 仍不发请求。B01、B05、B07、B08、B09 在这些页面上只作为依赖文字出现。live、journey、release 仍是 false。

## 下一刀

G1 继续补真正挂到 `/` 的对话页之前，需要一个真实的会话判断。在 B01 仍是 unbound 时，`/` 保持登录重定向。后续用户页仍然不造消息、不造会话、不把管理对象名露给普通身份。那一刀单独提交。
