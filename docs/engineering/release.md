# 发版手册

> Last reviewed: 2026-11-16（按 0.2.0 实际发版过程整理）

## 1. 产物和它们之间的关系

```text
master SHA
  ├─ lite-images.yml ──► ghcr.io/puppyone-ai/open-ontology/lite@sha256:<索引>   （多架构，cosign 无密钥签名 + SBOM + provenance）
  │                          │
  │                          └─ lite-bundle-release.yml ──► GitHub Release vX.Y.Z
  │                                                          oo-lite-runtime-vX.Y.Z-linux-{amd64,arm64}.tar.gz
  │                                                          lite-bundles.json（每个架构的大小、sha256、manifest、digest）
  │                                                          SHA256SUMS
  └─ cli-binaries.yml (lite_image=<索引>, lite_bundle_manifest=vX.Y.Z)
         ──► cli-npm-tarball：四个平台的 oo / mcp-gateway / oo-mysql-doctor
             + oo-lite-image（镜像 digest 固定值）+ oo-lite-bundle.json（离线包固定值）
                 ──► npm publish（由 owner 手动执行）
```

用户执行 `oo up` 时，按这个顺序获取运行时镜像：
1. 本机已有的镜像；
2. 之前加载过的离线包；
3. `--offline-bundle <文件>`；
4. 按 digest 执行 `docker pull`；
5. 从 Release 下载离线包，校验大小和 sha256、跑校验器、`docker load`，确认加载后的镜像 ID 和固定值一致。

设置 `OO_LITE_OFFLINE=1` 会关闭所有拉取和下载。PostgreSQL 镜像是 Docker Hub 上的公开镜像，不放进离线包。

## 2. 发版步骤

前提：master 上所有工作流的最新一次运行都是绿的，见 [ci.md](ci.md)。

1. **快进镜像分支**。`lite-images.yml` 只能从这个分支派发：
   ```bash
   git push origin origin/master:refs/heads/v0.2/product-loops
   ```
2. **构建并签名镜像**：
   ```bash
   gh workflow run lite-images.yml --repo puppyone-ai/open-ontology --ref v0.2/product-loops
   ```
   运行结束后，从 "Merge exact digests and sign keylessly" 这一步取多架构**索引** digest。验证顺序是 amd64、arm64、索引，最后一个就是索引。
3. **发布离线包**（在 master 上）：
   ```bash
   gh workflow run lite-bundle-release.yml --repo puppyone-ai/open-ontology --ref master \
     -f lite_image='ghcr.io/puppyone-ai/open-ontology/lite@sha256:<索引>' -f tag=vX.Y.Z
   ```
   确认 Release 已经发布（不是草稿或预发布），并且附件可以匿名下载。
4. **构建 CLI 并打包**：
   ```bash
   gh workflow run cli-binaries.yml --repo puppyone-ai/open-ontology --ref master \
     -f lite_image='ghcr.io/puppyone-ai/open-ontology/lite@sha256:<索引>' -f lite_bundle_manifest=vX.Y.Z
   gh run download <run-id> --repo puppyone-ai/open-ontology --name cli-npm-tarball --dir <目录>
   ```
   核对以下几项：
   - `package.json` 的版本号；
   - 四个平台下各有 `oo`、`mcp-gateway`、`oo-mysql-doctor`、`oo-lite-image`、`oo-lite-bundle.json`；
   - `THIRD_PARTY_NOTICES.txt` 包含静态链接进来的 OpenSSL；
   - 记下 tarball 的 sha256。
5. **验收**（在没有任何镜像仓库凭证的环境下）：
   - 用空的 HOME、全新的 npm prefix 安装 tarball；
   - `DOCKER_CONFIG` 指向一个不含 `config.json` 的目录（只放 compose 和 buildx 插件的软链接）；
   - 依次运行 `oo quickstart --yes --json`、`oo try --json`、`oo quickstart --down`；
   - 记录从安装到首答的耗时，确认没有残留资源。
6. **发布 npm**（由 owner 执行）：
   - 先用 `npm whoami` 确认登录有效。token 过期时 publish 会返回 **404**，不是 401。
   - 发布需要 WebAuthn 安全密钥。用伪终端运行 npm，把打印出来的 `https://www.npmjs.com/auth/cli/…` 链接交给 owner 在浏览器里确认。
   - 看到 `+ <包名>@<版本>` 且退出码为 0，才算发布成功。
   - 注册表元数据有几分钟延迟。之后用 `npm view <包>@<版本> dist.shasum` 核对 shasum 与本地 tarball 一致，再到干净目录里安装一次，跑 `oo --version`。
7. **台账**：在 `execution/` 中追加一行发版记录（版本、shasum、Release URL、镜像 digest），并把对应任务改为 `released`。

## 3. 镜像可见性

- 镜像设为**公开**是首选：用户直接 `docker pull`，升级时能复用已有的层。只有组织 owner 或包的管理员才能修改可见性，路径是"组织 → Packages → `open-ontology/lite` → Package settings → Change visibility"。
- 镜像还没公开时，用户会自动退回到 Release 离线包。仓库是公开的，Release 附件可以匿名下载。
- 不要把离线包放进 npm 包：完整离线包每个架构约 290 MB，超过 npm 的实际发布上限。npm 包里只放固定值。
- 发布前要确认镜像里没有密钥（密钥扫描），并且带有签名和 SBOM。

## 4. 已知注意事项

- npm 11.19 起默认不运行安装脚本，安装时会提示 `allow-scripts`，但这不影响使用。自检可以手动跑：`oo install-check --json`。后续计划改为第一次运行时自检。
- Linux 的 `oo-mysql-doctor` 静态链接了 OpenSSL。版本和 sha256 固定在 `ci/cli.Dockerfile` 中，审计记录在 `supply-chain/openssl-native-audit.json`。上游发布安全修复且满 7 天后就跟进升级。
