# 04 开源接入与 Gateway 绑定设计（闭环 L1 / L2）

> Status: Proposed
> Owner: Open Ontology Engineering
> Last reviewed: 2026-11-04
> 依赖：[07 Gateway](../07-gateway.md)、[08 Agent 接口](../08-agent-interface.md)、[10 安全](../10-security.md)、[11 部署](../11-deployment-and-operations.md)

## 1. 目标与现状差距

**目标体验。** 用户安装一个命令行工具，在 TUI 向导里填入数据库连接（可选 SSH 跳板）、大模型 key，可选填一台部署服务器的 SSH。之后整条链自动跑完：拉全量数据、Self-build、产出可视化 Ontology 与 Knowledge Pack、启动 Gateway、签发 token。最后屏幕上给出一段提示词，用户复制给自己的 Agent，Agent 按提示安装 CLI、绑定 token，用户随即可以和数据库对话。本期不做 Web UI。

**现状**（origin/master `6bbb368d4` 盘点）：

| 环节 | 现状 | 差距 |
|---|---|---|
| 消费端 CLI | `oo` 查询命令齐全；npm 包装层有 `configure`、`config set`、`time` | 无 `login`/`bind`、无续期、无 whoami 到期提示 |
| 接入向导 | 无；`oo` 设计上拒绝交互输入 token | 需新增向导，与消费端的“拒绝交互”约束分开 |
| 一条命令跑完整链 | 无；`tools/minimal-product-loop` 只支持 PG、只到未审 draft | 需新增编排入口 |
| SSH | 无；`tools/source-onboarding` 把 `ssh` 当非法输入 | 需新增跳板隧道与远程部署 |
| 模型配置 | 只接受 loopback 上的 `/v1/chat/completions`，模型白名单 | 需内置出口代理，用户可填任意兼容端点 |
| 规模 | Builder `MAX_TABLES=64` | 见 [03](03-selfbuild-context-budget.md) |
| 可视化 | 仅 `apps/web` 图组件 | 需离线 HTML 导出 |
| token | 运维用 `issue-token` 签发/吊销；无续期、无自助 | 需完整生命周期 |
| 部署 | `deploy/product-stack/compose` 镜像需自建，standard 档依赖 runsc | 需预构建多架构镜像与 `lite` 档 |

## 2. 命令面

一个 npm 包（现有 `packages/cli`）同时提供两类命令。消费端命令保持非交互、可管道；管理端命令可交互。

| 命令 | 角色 | 说明 |
|---|---|---|
| `oo init` | 管理端 | TTY 下进入 TUI 向导；非 TTY 或 `--config stack.yaml` 时完全非交互（CI、Agent 均可用） |
| `oo up` / `oo down` / `oo status` | 管理端 | 按 `stack.yaml` 启停本地或远程栈，显示每个阶段状态、最近成功、下次到期 |
| `oo doctor` | 管理端 | 只读预检：网络、版本识别、权限、日志/归档条件、预算估算，不写源库 |
| `oo build [--resume]` | 管理端 | 首次全量、Self-build、发布；可断点续跑 |
| `oo ontology open` / `export --format html|mermaid|yaml` | 管理端 | 打开或导出可视化 Ontology |
| `oo token issue|list|renew|rotate|revoke` | 管理端 | token 生命周期（§5） |
| `oo share-prompt [--role R]` | 管理端 | 生成给 Agent 的安装提示词（§6） |
| `oo login` | 消费端 | 用一次性绑定码换取长期 token，写入 0600 文件（§5.3） |
| `oo whoami` | 消费端 | 显示身份、角色、到期时间、Gateway 协议版本；到期 ≤7 天时 stderr 警告 |
| 现有 `query/tables/describe/...` | 消费端 | 不变 |

TUI 用 Rust `ratatui` + `crossterm` 实现（与 `oo` 同语言、同二进制，npm 包只做分发）。向导的每一步都对应 `stack.yaml` 的一个字段，向导结束时写出 `stack.yaml`，所以“TUI 填”与“文件填”是同一条路径。

## 3. 向导流程

```text
1 数据源      引擎(自动识别版本) / host / port / database|service / 用户 / 密码
              可选: SSH 跳板 (host, user, key 或密码, known_hosts 指纹确认)
2 范围        全部 schema / 选择 schema / 排除表; 默认全部
3 大模型      provider 预设(OpenAI 兼容 / Anthropic / 自定义 base URL) + key + 模型
              → 立即做一次最小调用验证; 显示上下文窗口与输出上限(可手改)
4 部署位置    本机 Docker (默认) / 远程服务器 (SSH host, user, key)
5 预检        oo doctor 的结果: 版本、权限、行数与字节估算、分块计划、
              模型 token 与费用估算、预计耗时; 有阻断项时停在这里并给出 DBA 清单
6 确认并运行  进度面板: 每个阶段(拉取/装载/Self-build/审核/发布/Gateway)的
              进度条、吞吐、失败与重试; 可退出, 稍后 oo status / oo build --resume
7 完成        Ontology 可视化路径, Pack 路径, Gateway URL, 首个 token,
              以及可复制的 Agent 提示词
```

**秘密处理。** 向导读入的密码、key 直接写入 `~/.config/open-ontology/secrets/<name>`（0600，目录 0700）或系统 keychain；`stack.yaml` 只存 `secret_ref`。不出现在 argv、环境变量导出、日志、进度面板里。这延续 `tools/source-onboarding` 的“connection 只接受 secret_ref”约束。

**审核门。** 产品发布门禁要求审核。开源单人场景下，向导提供“所有者自审”模式：Self-build 产出后在 TUI 展示待审项摘要（实体、关系、指标、敏感列分类），所有者逐类确认或批量接受；接受记录写成 `review_decision`，标注 `reviewer=owner, mode=self`。不允许跳过审核直接发布，也不把自动接受伪装成人工审核。

S4 实现边界：`oo review` 支持所有者逐项自审、G14 重算和 `--review-file` 重放。`oo build` 的 draft/review 交接可恢复，但旧草稿到 community 输入的适配及发布后的 Gateway/token/prompt 阶段仍未完成；`--draft-only` 保留仅生成草稿的路径，不能作为完整上线验收。

## 4. 编排与部署

### 4.1 `stack.yaml`

```yaml
version: oo.stack/v1
sources:
  - id: erp
    engine: auto            # 预检时识别为 mysql-5.7 / mysql-8.0 / oracle-19c ...
    host: db.internal
    port: 3306
    database: erp
    user: oo_reader
    password: { secret_ref: erp-password }
    ssh: { host: bastion.example.com, user: ops, key: { secret_ref: bastion-key }, host_key_sha256: "..." }
    scope: { include: ["*"], exclude: [] }
    sync: { mode: auto, interval: 8h }   # auto = 有日志条件用 C1, 否则 C5
model:
  provider: openai-compatible
  base_url: https://api.example.com/v1
  model: some-model
  key: { secret_ref: model-key }
  context_window: auto      # 首次调用后探测, 可覆盖
deploy:
  target: local             # local | ssh
  profile: lite             # lite (单机 compose, 无 runsc) | standard
gateway:
  public_url: auto          # local 时为 https://localhost:<port>
```

### 4.2 运行形态

- **lite 档**（默认，开源上手）：首次 `oo up` 自动生成内部 CA、Gateway 与组件间 mTLS 证书（有效期 2 年，到期监控见 §5.2）和管理员凭据，全部写入 0600 秘密目录；单机 Docker Compose，组件为 Agent PostgreSQL、控制库、worker、Builder、Gateway、模型出口代理。不要求 runsc；内置 Pi 沙盒不启用。安全边界写在文档里：lite 档面向单所有者，不承诺多租户隔离。lite 档内 Agent DB 与控制库只在 compose 内部网络暴露，连接使用 TLS（校验服务端证书）+ SCRAM 口令，不要求客户端证书；Gateway 与内部服务之间的 mTLS 不变；standard 档仍要求客户端证书（Plan D14）。
- **standard 档**：现有 `deploy/product-stack`，包含身份、审计、沙盒门。

镜像由 CI 预构建为 `linux/amd64` 与 `linux/arm64` 多架构，用 digest 锁定；`oo up` 只拉取，不在用户机器编译。

### 4.3 SSH 的两种用途

| 用途 | 实现 | 约束 |
|---|---|---|
| 数据库跳板 | worker 内用 `russh` 建 direct-tcpip 通道，每个读连接一个通道 | 首次连接显示主机指纹并要求确认，写入 `host_key_sha256`；之后不匹配即失败；不改全局 SSH 配置；keepalive 与断线重连按分块断点续读 |
| 远程部署 | `oo up` 经 SSH 在目标机安装 compose 栈与 systemd 单元，日志与状态通过 SSH 读取 | 只写约定目录；需要 Docker 时检测并给出安装指引，不静默 `sudo` |

反向通道（把本地 Gateway 暴露到公网）不属于默认路径；远程部署本身就提供可达的 Gateway。

### 4.4 模型出口代理

Builder 的 loopback-only 约束保留（安全设计：Builder 不持有外网凭据）。栈内新增 `model-egress` 容器：监听 loopback，持有 key，向用户配置的 base URL 转发；负责 provider 方言转换（OpenAI 兼容 / Anthropic Messages）、限流、`usage_limit`/`cooldown` fail-fast 的统一错误码、调用计量。模型白名单改为“任意模型 + 首调探测”，探测结果（上下文窗口、最大输出、是否支持 JSON 模式/工具调用）写入运行记录，供 [03](03-selfbuild-context-budget.md) 的预算器使用。

### 4.5 持续同步与发布跟随

`oo up` 默认安装周期任务（compose 内 scheduler；远程用 systemd timer）。每个周期：增量（C1 或 C5）→ 发布新数据代际 → Gateway 原子切换发布指针。只有结构或语义依赖变化时才触发定向 Self-build。这就是“实时匹配”的含义：Gateway 始终服务最近一次已验证的发布，并通过 `/v1/freshness` 告诉调用者数据截至何时；Gateway 不直连源库。

## 5. Token 生命周期（闭环 L2）

### 5.1 现有基础

`tools/gateway-token-store`：不透明 token（`ooct_` client / `oost_` session，32 字节 base64url），库里只存 SHA-256；记录 principal、role、claims、policy/knowledge 版本、audience、idle/absolute deadline；吊销粒度 token/session/principal/project。签发由 `crates/oo-gateway/src/bin/issue-token.rs` 完成，需 admin 密码文件与 CA。

### 5.2 新增操作

| 操作 | 语义 | 审计 |
|---|---|---|
| `issue` | 新 token，指定角色、有效期（默认 90 天；最大寿命由项目策略定，默认上限 90 天、可配置至 365 天） | 记录签发人、用途 |
| `renew` | **同一 token 值**延长 absolute deadline，不超过策略上限；被吊销或已过期的不可 renew | 记录旧/新到期；这是正式 API，取代此前的手工改表 |
| `rotate` | 签发新值，旧值进入重叠窗口（默认 7 天）后自动失效 | 记录新旧关联 |
| `revoke` | 立即失效 | 原有 |
| `list` | 列出 hash 前缀、角色、到期、最后使用时间 | 不输出明文 |
| `enroll` | 签发一次性绑定码（默认 24 小时、只能兑换一次），兑换时生成 client token | 记录兑换来源 |

自助续期：策略允许时，持有 token 的 CLI 可调用 `POST /v1/auth/renew`，在到期前 N 天内把 deadline 延到不超过“首次签发 + 最大寿命”。管理员可关闭自助续期。

Gateway 内部 mTLS 证书与 token 一起纳入到期监控：`oo status` 和 `/v1/health` 报告最近到期项，≤14 天警告，过期即标红。

### 5.3 CLI 绑定

```text
oo login --gateway https://gw.example.com --code OO-7F3K-Q9XM
  → POST /v1/auth/enroll {code}         (TLS 校验, 不允许关闭)
  → 返回 client token + 元数据
  → 写 ~/.config/open-ontology/tokens/<profile>.token (0600)
  → profile 记录 gateway、token 文件引用、角色、到期、业务时区
  → 自动跑一次 oo whoami 与 oo tables --limit 1 作为绑定验证
```

非交互：`oo login --gateway URL --code-stdin`。已有 `configure --token-file` 路径保留，供离线或运维直配。

**设备码方式**（RFC 8628，与 `gh auth login` 相同）：`oo login --gateway URL --device` 打印验证地址和一次性码，管理员在 `oo token approve <code>`（或远程部署主机的 TUI）确认角色后，CLI 轮询拿到 token；轮询遵守 `slow_down` 加 5 秒的规定。适合用户与管理员不是同一人、或不方便传递绑定码的场合。绑定码方式仍是默认，因为单人开源场景下用户本人就是管理员。

**短期 access + 轮换 refresh token**（业界常见做法，例如 GitHub App 8 小时 access / 6 个月 refresh）不在 v0.2 范围：现有 token-store 是不透明 token + idle/absolute deadline，v0.2 用 renew/rotate/到期预警补齐生命周期；v0.3 再评估 refresh 轮换与重放检测。

### 5.4 协议协商

`GET /v1/meta` 返回 Gateway 版本、协议版本、最低兼容 CLI 版本、当前发布与新鲜度。CLI 每次启动缓存检查（24 小时一次）：低于最低版本时给出升级命令并以明确错误码退出，而不是在查询中途出现语义错误。

## 6. Agent 提示词

`oo share-prompt --role analyst` 输出一段纯文本，用户整段复制给 Agent。另提供 `oo share-prompt --agent claude|cursor|codex`，在提示词中加入对应 Agent 的 MCP 配置步骤（参考 `neonctl init` 一条命令配齐 Agent 的做法；MCP 网关已有 `tools/mcp-gateway`）。

```text
请帮我配置 Open Ontology 数据访问:
1. 安装 CLI: npm install -g <发布包名>@<精确版本>
2. 绑定: oo login --gateway https://gw.example.com --code-stdin <<< 'OO-7F3K-Q9XM'
   (绑定码 24 小时内有效, 只能使用一次)
3. 验证: oo whoami && oo tables --limit 5
4. 读取使用说明: oo agent-skill
之后回答我的数据问题时: 先 oo describe 了解表和实体, 再用 oo query;
结果被截断、为 NULL 或查询失败时如实说明, 不要当成 0。
```

默认放一次性绑定码而不是长期 token：用户要的“提示词里带 token”成立，同时聊天记录里不留下可长期使用的凭据。`--raw-token` 可改为直接嵌入长期 token，此时提示词顶部加一行风险说明。包名与版本由发布流程填入，不写 `latest`。当前已发布包为 `@reduxxx888/ontology-cli`；开源发布前是否迁到组织作用域包名由人类决定（Plan 待决项）。

## 7. 验收要点

1. 干净机器（macOS arm64 与 Linux amd64 各一）上：安装 → `oo init` → 本地 Docker 中的 MySQL/PG/Oracle 测试库 → 完成页给出提示词 → 另一个 Agent 按提示词绑定并回答一组固定问题，全程不手工编辑任何文件。
2. 同一流程用 `stack.yaml` 非交互复现，结果一致。
3. 经 SSH 跳板访问数据库、部署到远程 Linux 主机各通过一次（本期目标主机为本机起的 sshd + Docker 容器，云主机就绪后补真机）；指纹不匹配时拒绝连接。
4. 秘密扫描：argv、日志、`stack.yaml`、Pack、提示词（`--raw-token` 除外）中无明文秘密。
5. token：issue/renew/rotate/revoke/enroll 每项有成功与失败用例（过期、已吊销、重复兑换、超最大寿命、错误 audience）；CLI 在到期前警告，过期后给出续期指引。

## 8. 外部参考（2026-11-03 检索）

- RFC 8628 设备授权：https://www.rfc-editor.org/info/rfc8628/
- neonctl init 一条命令配置 Agent：https://neon.com/docs/reference/cli-init
- GitHub App token 过期与刷新：https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/refreshing-user-access-tokens
- ratatui / inquire / Bubble Tea 对比：https://deska.dev/blog/ratatui-bubbletea-tui-frameworks
