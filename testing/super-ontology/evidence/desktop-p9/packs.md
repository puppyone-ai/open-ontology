# 阶段二 owner 验收包（60 §3.4、§4）· P9 轮构建记录（2026-11-18）

构建方式：按仓库的发布脚本，验收渠道、本地无签名、不公证、不上传。
`node scripts/prepare-desktop-build.mjs --channel acceptance`，再 `npm run dist:mac:prepared`（`CSC_IDENTITY_AUTO_DISCOVERY=false`；Cloud 地址取 `.env.example` 的公开值）。产物是 ad-hoc 签名的 arm64 DMG，没有用到任何证书或密钥。包由 `scripts/build-acceptance-pack.mjs` 组装，放在仓库外。

| 项 | 值 |
|---|---|
| 应用 | `PuppyOne Acceptance`，`ai.puppyone.desktop.acceptance`，版本 `0.3.27-acceptance.g23ca7546` |
| 构建源 | Puppyone `sd/P9-evidence` 的 `23ca7546`（`codex/super-drive` `f2ddc8cc` 之上一条测试提交，工作树干净） |
| DMG（原始产物） | `/Users/redux/puppy-one/puppyone-wt/sd-P9/release/puppyone-0.3.27-acceptance.g23ca7546-arm64.dmg`（构建完成后随工作树清理而删除，交付副本在下面三个包里） |
| DMG sha256（三个包相同） | `c58862a2f5c52f6dad194d7354432939f7df8999c7ff64cf1a428955931607c9` |
| 引擎二进制 | `engine/oo` sha256 `ebd5a9f0c245aabea4ece84e0326219dfc10e8f7048a2bf4df6f60236b16706a`、`engine/oo-docs-engine` sha256 `b6f9efd597191818af1fbe622824de14212e560d4afa980b881bae2744e64521`（OO `406dd0dc6`，debug，`--features docs` / `objectstore-s3`） |
| 契约 | docs-api 1.9 |

| 包 | 路径 | 内容 |
|---|---|---|
| L4 | `~/Desktop/super-ontology-acceptance/L4/` | `PuppyOne-super-drive-L4.dmg`、`BUILD.txt`、`ACCEPTANCE.md`（1–17 步）、`sample-drive/`、`engine/`、`evidence/t1/`、`KNOWN-ISSUES.md`、`FEEDBACK.md` |
| L6 | `~/Desktop/super-ontology-acceptance/L6/` | 同上，清单追加 18–25 步（Suggest、漂移刷新、OCR、媒体、说话人、模型） |
| L8 | `~/Desktop/super-ontology-acceptance/L8/` | 同上，清单再追加 26–30 步，另有 `sample-drive-10k/` |

**这三个包不满足 60 §3.4 的交付前提**：T2 没有运行、T3 没有做，且打包应用的启动在构建机上没能确认（主进程空转，没有窗口与输出，原因未查明）。包里的 `KNOWN-ISSUES.md` 如实写了 9 条。L4、L6、L8 包用的是同一个 DMG，不是各里程碑当时的构建。
`oo` 不随 App 打包（App 只在 PATH 与 `/usr/local/bin`、`/opt/homebrew/bin` 里找），所以包里带了 `engine/`；owner 需要把 `oo` 拷到 `/usr/local/bin`，这一步不能完全免终端。
