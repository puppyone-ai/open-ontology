# Super Ontology 商用边界

> 状态：**C1–C10 已由 owner 于 2026-11-17 确认**；对外发布前须经律师审阅（C10、§8）。
> 范围：本文只定义商用边界、组件准入、分发义务与 AI 标识。**用户须知（隐私说明、使用条款等）由官网发布，不在本 spec 范围内，也不在 App 内展示。** 许可证要求随产品分发的开源许可署名（§5）与 AI 生成内容标识（§7）属于分发与法规义务，不是用户须知，仍在本文范围内。
> 本文不是法律意见。核准方式：许可证全部于 2026-11-17 直接读取 GitHub raw 文件、Hugging Face / ModelScope API、PyPI / crates.io 元数据；法规以官方原文或权威转载为准，未读到一手原文的标“未核”。
> 判定依据：仓库 [`supply-chain/policy.md`](../../supply-chain/policy.md)：默认产物只收 OSI 宽松许可；弱 copyleft（LGPL、MPL）须单独审查；AGPL、BUSL、Elastic-2.0、SSPL 禁止；GPL 一律排除；未知许可、非 OSI、`LicenseRef-*` 默认排除。模型权重与数据集保留各自许可与署名。

## 1. 边界原则

| # | 原则 | 为什么 |
|---|---|---|
| C1 | **本地默认模式下，我们只是工具提供方**：文件内容、转写、本体、BRIEF 都只在用户设备上；我们不收集、不中转、不代理任何文件内容 | 用户（或其机构）是个人信息处理者 / 控制者；我们只对自己收集的账号、授权、遥测数据承担处理者义务（个保法第 72、73 条；GDPR 序言第 18 条、EDPB 07/2020） |
| C2 | **不内置模型 API Key，不提供经我们中转的模型服务**；模型预设只列服务商名称，用户自填自己的 Key 与地址 | 一旦由我们中转或代付，我们就可能成为数据出境方、受托方，甚至“生成式 AI 服务提供者”（暂行办法第 2、22 条） |
| C3 | **遥测默认关闭**，开启后只收匿名计数与崩溃堆栈，内容可在本地查看 | 个保法第 6、13 条；ePrivacy 第 5(3) 条（未核） |
| C4 | **本轮不做声纹库**（跨录音识别同一个人）。说话人分离只在单条录音内临时使用声音特征，处理完即丢弃 | 声纹是敏感个人信息（个保法第 28、29 条，GB/T 35273 6.3）；BIPA 有集体诉讼风险；跨录音识别可能落入 EU AI Act 的远程生物识别（附件 III 高风险）。等律师结论后单独立项 |
| C5 | **本轮不提供云端 ASR 和云端版内容处理**；L7 的 Cloud 代理模式在律师放行前不对外发布 | 云端处理会让我们变成受托方 / 处理者，并牵出备案、出境、DPA 等一整套义务 |
| C6 | **AI 生成内容必须标识**：BRIEF 等生成文件带显式“AI生成”标识和机器可读的隐式标识，产品内不提供去除标识的功能 | 《人工智能生成合成内容标识办法》第 4、5、10 条；GB 45438-2025；EU AI Act 第 50(2) 条（2026-08-02 起适用） |
| C7 | **默认产物只放许可证核准为“默认可用”的组件**；有条件的组件须满足条件并留档；禁用组件由 SBOM 门禁拦截 | 本仓库供应链政策 |
| C8 | **不随产品分发有专利费风险的解码器**：H.264、H.265、AAC 不在我们分发的 FFmpeg 里；这类文件交给用户设备的系统解码器 | Via LA 的 AVC、AAC 许可对“PC 软件”终端产品收费，AAC 没有免费额度 |
| C9 | **不面向 14 周岁以下用户** | 个保法第 31 条 |
| C10 | **对外发布前必须过律师门禁**：§9 的问题有书面结论后，才能发布带 Super Drive 的公开版本或提交 Puppyone 最终 PR | 风险集中在少数几个问题上，必须先有结论 |

## 2. 组件准入表

“默认可用”= 可进默认容器镜像与离线包；“有条件”= 满足条件列的要求才可进；“外部可选”= 不进我们的产物，用户或客户自行安装、自行承担许可；“禁用”= 门禁拦截。

### 2.1 文档与图片

| 组件 | 代码许可 | 权重许可 | 判定 | 条件或说明 | 证据 |
|---|---|---|---|---|---|
| docling、docling-core、docling-parse、docling-ibm-models | MIT | — | 默认可用 | 保留版权声明 | `docling-project/*/LICENSE` |
| docling-layout-heron（默认版面模型） | — | Apache-2.0 | 默认可用 | 附 Apache 文本 | HF API |
| docling-models（TableFormer） | — | CDLA-Permissive-2.0 与 Apache-2.0 并标 | 有条件 | 两份许可文本都随包附上；律师确认按 Data 还是 Results 处理（§9 L5） | HF API、README |
| DocumentFigureClassifier-v2.5 | — | MIT | 默认可用 | 默认不启用 | HF API |
| SmolVLM-256M-Instruct（图片描述） | — | Apache-2.0 | 有条件 | 基座 SmolLM2-135M、SigLIP 的许可须先核；默认不启用 | HF API |
| RapidOCR（onnxruntime 版） | Apache-2.0 | Apache-2.0（ModelScope） | 默认可用 | **Docling 的 OCR 必须显式指定 RapidOCR**，不用 `OcrAutoOptions` 自动选择 | GitHub LICENSE、ModelScope API |
| Nemotron OCR（Docling 的 `feat-ocr-nemotron`） | — | NVIDIA Open Model License（非 OSI） | **禁用** | 不安装该 extra | HF 页面 |
| EasyOCR | Apache-2.0 | 未核 | 不进默认产物 | 依赖 LGPL 的 python-bidi，且权重未核 | GitHub LICENSE |
| PaddleOCR 3.7 / PaddleX 3.7 / PaddlePaddle | Apache-2.0 | — | 有条件 | 引入的 python-bidi（LGPL）、cssutils（LGPL-3.0+）须过弱 copyleft 审查；PaddleX 的云 SDK 在容器里被网络隔离屏蔽，并须关闭自动下载 | 各仓库 LICENSE、setup.py |
| PP-DocLayout_plus-L、PP-OCRv5_server_det/rec、SLANeXt_wired | — | Apache-2.0 | 默认可用 | 其余表格子模型（RT-DETR-L cell det、PP-LCNet table_cls）须逐个核后才放行 | HF API |
| PaddleOCR-VL-1.5（GPU 档） | — | Apache-2.0 | 默认可用 | 基座许可须先核；只在 `gpu` 档启用 | HF API |
| MinerU（代码）、MinerU2.5-Pro（权重 Apache-2.0） | MinerU Open Source License（非 OSI，附规模门槛与署名义务） | Apache-2.0 | **外部可选** | 不进我们的产物 | `MinerU/LICENSE.md` |
| Marker、Unstructured 开源版、Morphik（BUSL）、MarkItDown 音频（需联网） | — | — | **禁用 / 不采用** | 见 40 分册 | — |
| calamine、pulldown-cmark | MIT | — | 默认可用 | — | crates.io |
| OpenDAL | Apache-2.0 | — | 默认可用 | 附 NOTICE | crates.io |
| blake3 | CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception | — | 默认可用 | 选择 Apache-2.0 | crates.io |
| RustFS | Apache-2.0 | — | 默认可用（测试与客户自建） | 商标仅作名称引用 | GitHub LICENSE、README |

### 2.2 音频与视频

| 组件 | 代码许可 | 权重许可 | 判定 | 条件或说明 | 证据 |
|---|---|---|---|---|---|
| Qwen3-ASR-0.6B / 1.7B、Qwen3-ForcedAligner-0.6B | 仓库 Apache-2.0 | Apache-2.0，不门控 | 权重默认可用 | **不得使用 `qwen-asr` PyPI 包**：它强制依赖 GPL-3.0 的 soynlp。用 transformers 或自写推理代码加载权重 | HF API、PyPI JSON |
| FireRedASR2S：AED、VAD、LID | Apache-2.0 | Apache-2.0 | 默认可用 | LLM 子模型（内含 Qwen2-7B-Instruct）与 Punc（内含 chinese-lert-base）未核，不启用 | GitHub、HF API |
| FunASR（代码） | MIT | 权重另行许可 | 默认可用 | — | `FunASR/LICENSE`、README |
| paraformer-zh、fsmn-vad、ct-punc、CAM++ | — | Apache-2.0（HF 与 ModelScope 一致） | 默认可用 | 保留模型名 | HF API、ModelScope API |
| SenseVoiceSmall | MIT | HF 标 FunASR MODEL_LICENSE v1.1，ModelScope 标 Apache-2.0，**两边冲突** | **按排除处理** | 律师给出结论前不进产物（§9 L1） | HF API、ModelScope API |
| sherpa-onnx | Apache-2.0 | 每个模型各自许可 | 默认可用 | 只用 3D-Speaker 的 ONNX 模型；wespeaker、nemo 模型未核，不用 | GitHub LICENSE、release 页 |
| 3D-Speaker（ERes2Net、CAM++） | Apache-2.0 | Apache-2.0 | 默认可用 | — | GitHub LICENSE、ModelScope API |
| pyannote.audio 4.0.7 | MIT | — | 有条件 | 必须设 `PYANNOTE_METRICS_ENABLED=0` 关闭其遥测；依赖 pyannoteai-sdk 的许可未核 | GitHub LICENSE、README |
| pyannote/segmentation-3.0 | — | MIT，HF 门控 | 有条件 | 构建期由我们拉取一次 | HF API |
| speaker-diarization-community-1 | — | CC-BY-4.0，HF 门控（要求填写单位、用途并同意营销邮件） | 有条件 | 署名；内含的 WeSpeaker 嵌入模型许可未核；门控条款下的再分发须律师确认（§9 L3）。**默认栈用 sherpa-onnx + 3D-Speaker，pyannote 只作比测对照** | HF API |
| DiariZen | MIT | CC-BY-NC | **禁用** | 非商用 | — |
| silero-vad 6.2.3 | MIT | MIT | 默认可用 | README 徽章写 CC BY-NC，与 LICENSE 不一致；以 LICENSE 为准并留存快照 | GitHub LICENSE |
| PySceneDetect 0.7.3 | BSD-3-Clause | — | 有条件 | 改用 headless 版 OpenCV，且 OpenCV 不得带 FFmpeg（见 §4） | GitHub LICENSE、PyPI |
| imagehash | BSD-2-Clause | — | 默认可用 | — | GitHub LICENSE |
| opencv-python-headless 官方 wheel | Apache-2.0 | — | **不进默认产物** | 官方 wheel 捆绑完整 FFmpeg（含 H.264 等解码器）；改用不带 videoio-FFmpeg 的自编 OpenCV | `opencv-python/README.md` |
| Qwen3-VL-2B / 4B-Instruct | — | Apache-2.0 | 默认可用 | 只在 `gpu`、`host-accel` 档并经用户开启；仓库无 LICENSE 文件，须自附 Apache 文本 | HF API |
| Qwen2.5-Omni-3B 等研究许可模型；Gemma、OpenRAIL、Llama 系 | — | 研究用或带使用限制 | **不进离线包** | 若将来作为外部可选，须在 EULA 传导使用限制（§7.12） | 许可原文 |

### 2.3 开发工具（不进产品）

Quint、Apalache（Apache-2.0）、Kani（MIT OR Apache-2.0）：仅开发与 CI 使用。

## 3. 传递依赖与 SBOM 门禁

- **已发现须处理的传递依赖**：
  - GPL-3.0：soynlp（由 qwen-asr 引入，已通过不使用 qwen-asr 规避）；extract-msg（只在 mail-parser 的 outlook extra 下出现，锁文件须证明未安装）。
  - LGPL：python-bidi、cssutils（由 PaddleX 引入）、OpenCV 捆绑的 FFmpeg；pypdfium2 附带的 37 份许可文件须全部随包分发。
  - 其他：chardet 须锁定在 7.x（0BSD）；SoX 二进制许可未核（随 qwen-asr 一起排除）。
- **未发现**：Docling、PaddleX、RapidOCR 的直接依赖中没有 PyMuPDF、pymupdf4llm、ultralytics、ghostscript。二级以下依赖须由 SBOM 证明。
- **门禁**：每个容器镜像构建后用 syft 生成 SBOM，用 pip-licenses / cargo-deny 复核；出现 AGPL、GPL、BUSL、SSPL、Elastic-2.0、CC-BY-NC、`LicenseRef-*` 即构建失败；LGPL 组件必须在 `supply-chain/` 有弱 copyleft 审查记录，否则失败。这条进 30 §4 横切门禁，从 L3 起生效。

## 4. FFmpeg 与编解码专利

- **我们分发的 FFmpeg**：自行编译 LGPL 版，不启用 `--enable-gpl`、`--enable-nonfree`；只启用解码器，并关闭 h264、hevc、aac 等有专利费风险的解码器；保留 Opus、Vorbis、FLAC、PCM、MP3（相关专利已过期，律师复核）、VP8、VP9、AV1（dav1d）。整个产品只解码、不编码。
- **LGPL 义务**：动态链接；镜像内 `/usr/share/licenses/ffmpeg/` 放 LGPL 文本、configure 行和源码获取方式；源码 tar 包作为同一仓库的发布制品提供；EULA 中不得禁止为调试而做的逆向工程；CI 检查 `ffmpeg -version` 的 configuration 行。
- **H.264、HEVC、AAC 文件（大多数 mp4、m4a、mov 会议录音录屏）**：
  - **macOS**：在宿主机上由 Desktop 主进程调用系统框架（AVFoundation）解码，转成 16 kHz WAV 或 FLAC 音频、加上关键帧 PNG，再交给容器处理。系统解码器的许可由操作系统承担。
  - **Linux 服务器与企业自建**：由客户自带 FFmpeg（“BYO FFmpeg”，配置路径即可），我们只调用不分发。
  - 都不满足时：文件进 `unsupported(DOCS_PARSE_CODEC_UNAVAILABLE)`，状态说明里写明原因和配置方法。
- **待律师确认**：只解码是否仍有 AVC、AAC 专利义务；客户自带 FFmpeg 的安排能否把责任转给客户；MP3 是否可放心启用（§9 L6）。

## 5. 模型分发与署名

- 模型不打进 Desktop 安装包；随 OO 的离线运行时包或首次启用时从**我们控制的制品源**下载，每个文件都有 sha256 锁定（`models.lock.json`）。运行时离线（`HF_HUB_OFFLINE=1`）。
- 门控模型（pyannote 系）若最终采用，由我们在构建期拉取一次；是否可以再分发须先有律师结论。
- 产物内放 `THIRD_PARTY_NOTICES/` 目录；Desktop 的“关于 → 开源许可”页面链接到它（这是许可证要求的署名，不是用户须知）。按许可类型分组：
  - MIT、BSD：LICENSE 全文与原版权行（Docling 四个包、calamine、pulldown-cmark、FunASR、pyannote.audio、silero-vad、segmentation-3.0、DocumentFigureClassifier、PySceneDetect、imagehash）。
  - Apache-2.0：LICENSE 与 NOTICE（OpenDAL、blake3、RustFS、RapidOCR、Paddle 系、sherpa-onnx、3D-Speaker、Qwen3-ASR / Aligner / VL、FireRedASR2S、Heron、SmolVLM、PP 系模型、FunASR 的四个模型并保留模型名、OpenCV）。
  - CDLA-Permissive-2.0 加 Apache-2.0：TableFormer。
  - CC-BY-4.0（如采用）：speaker-diarization-community-1，写明作者、许可链接、免责声明、是否修改。
  - LGPL：FFmpeg（附 configure 行与源码获取方式）、python-bidi、cssutils、pypdfium2 的全部许可文件。
  - AV1、Opus：AOM Patent License 1.0、Opus 的 BSD 文本。

## 6. 义务矩阵

| 场景 | 我们的角色 | 中国 | 欧盟 | 美国 | 产品必须做 |
|---|---|---|---|---|---|
| **本地默认** | 工具提供方；只对遥测、账号数据是处理者 | 用户或其机构是处理者（个保法第 73 条；纯个人事务适用第 72 条豁免） | 用户是控制者或适用家庭豁免；我们承担工具方责任（序言第 18 条）；AI Act 第 50(2) 条提供者义务 | 录音同意由用户负责 | AI 生成内容标识；遥测默认关；告知类内容由官网须知承担 |
| **用户自配境内模型** | 工具提供方 | 用户委托模型服务商（个保法第 21 条），模型服务商负责备案 | 同上 | — | 默认关闭；界面显示所选服务商与地区；可排除音视频转写与人员档案 |
| **用户自配境外模型** | 工具提供方，不中转 | 出境方是用户或其机构（个保法第 38、39 条；《促进和规范数据跨境流动规定》第 5、7、8 条） | 向第三国传输由用户依第五章处理 | — | 默认关闭；界面显示服务商所在地区；可排除音视频转写与人员档案；出境告知由官网须知承担 |
| 声纹库、云端 ASR、云端版 | **本轮不提供（C4、C5）** | — | — | — | — |

## 7. AI 生成内容标识的实现规格

- **显式标识**：BRIEF.md 第一行固定为 `> AI生成 · 由 Super Ontology 根据您的文件自动整理，可能有误，请以原文为准。`（英文界面为 `AI-generated …`），之后才是覆盖情况与一致性承诺；INDEX.md、人员与项目等生成文件同样在首行标注。界面上展示 BRIEF 预览时，顶部有“AI生成”角标。
- **隐式标识**：每个生成的 Markdown 文件带 YAML front matter：

```yaml
---
AIGC:
  Label: "1"                      # 1 = AI 生成
  ContentProducer: "<公司统一社会信用代码或名称>"
  ProduceID: "<pack_id>/<file>"
  ReservedCode1: "<pack_digest>"
  ContentPropagator: ""
  PropagateID: ""
  ReservedCode2: ""
---
```

  `SHARE.json` 与 `manifest.json` 增加同结构的 `aigc` 字段。字段名按 GB 45438-2025 附录 E 的元数据扩展字段设计；标准原文尚未逐字核对，实现前须以标准原文为准（§9 L2）。
- **不可去除**：产品内不提供去除标识的选项；`oo context mount` 也没有关闭标识的参数。用户自行编辑文件属于用户行为。
- **确定性**：标识内容不含时间戳，不影响 Pack 逐字节可重放（D27）。

## 8. 律师门禁

以下问题在**对外发布带 Super Drive 的版本**和**提交 Puppyone 最终 PR** 之前必须有书面结论（C10）。内部开发与 owner 验收不受影响。

| # | 问题 |
|---|---|
| L1 | SenseVoiceSmall 两个平台的许可证冲突以哪个为准？FunASR MODEL_LICENSE v1.1 第 3 条“仅供参考学习”是否构成商用限制，维护者在 README 中的澄清能否依赖？第 4、6、7 条（贬损即失效、自动修订、准据法空白）的风险？ |
| L2 | 本地运行、用户自配模型的软件，是否属于《标识办法》第 2 条的“网络信息服务提供者”？应用商店依第 7 条审核时如何说明？Markdown 文件的隐式标识按 §8 的 YAML front matter 实现是否满足 GB 45438-2025？ |
| L3 | pyannote speaker-diarization-community-1（CC-BY-4.0）：打进离线包的署名格式；镜像签名或加密是否构成 §2(a)(5)(B) 的“有效技术措施”；绕过 HF 门控再分发有无问题？ |
| L4 | 单条录音内临时生成、不保存、不识别身份的说话人特征，是否构成个保法第 28 条的生物识别信息处理、GDPR 第 9 条处理、BIPA 的 voiceprint？ |
| L5 | TableFormer 权重同时标 CDLA-Permissive-2.0 与 Apache-2.0，按哪一个处理？ |
| L6 | 只解码不编码，是否仍需 AVC、AAC 专利许可？macOS 上调用系统解码器、Linux 上由客户自带 FFmpeg 的安排，是否足以让我们不承担专利义务？MP3 解码器能否放心启用？ |
| L7 | python-bidi、cssutils、OpenCV / FFmpeg 等 LGPL 组件在容器镜像中的分发方式（动态链接、可替换、源码提供方式）是否合规？ |
| L8 | 本地默认模式下，“自动抽取人员档案”的产品设计是否会使我们被认定为个人信息处理者，或需要承担 GDPR 序言第 18 条式的设计义务？ |
| L9 | 我们在欧盟是否属于第 50(2) 条“生成文本的 AI 系统”提供者？是否适用“标准编辑辅助”例外？首次在欧盟投放时间对第 111(4) 条宽限期的影响？ |
| L10 | 用户自配境外模型时，“默认关闭 + 官网须知”是否足以让我们不承担出境义务？若提供服务商预设列表（不含 Key、不中转），角色是否改变？ |
| L11 | App 内不做录音同意确认、只在官网须知中要求用户取得同意，在 CIPA §631/§632 下我们被诉为“第三方窃听者”的风险如何（参照 In re Otter.ai Privacy Litigation）？纯本地模式下的风险评估？是否建议至少在 App 内保留一处确认？ |
| L12 | 遥测默认关闭、匿名计数，在中国是否仍需单独的处理规则？设备 ID 是否算个人信息？ |
| L13 | 官网须知与 EULA 中“AI 准确性免责、不得作为对他人决策唯一依据”的条款，在中国格式条款规则下的效力，对消费者用户是否有限制？须知只放官网、App 内不展示，告知是否充分（个保法第 17 条“显著方式”）？ |
| L14 | 将来做声纹库（C4）或云端版（C5）时的前置合规清单：单独同意的取证、影响评估、BIPA 书面授权与公开保留政策、AI Act 远程生物识别判定、生成式 AI 备案或登记、DPA 与数据驻留。 |

## 9. 对其他分册的影响（已同步）

- 00：新增决策（商用边界 C1–C10 转为 D 编号）；§6.3 Pack 目录加 AIGC 标识；不做声纹库与云端 ASR 写入范围。
- 10：解析层按 §2 选型；FFmpeg 与宿主机解码路径（新增 `DOCS_PARSE_CODEC_UNAVAILABLE`）；Docling 显式使用 RapidOCR；不使用 `qwen-asr` 包；SBOM 门禁；AIGC 标识写入编译器。
- 20：macOS 宿主机解码服务（主进程原生 helper）；说话人确认界面；“关于 → 开源许可”入口；BRIEF 预览的“AI生成”角标。App 内不展示用户须知。
- 30：横切门禁增加“SBOM 许可门禁”（L3 起）与“律师门禁”（对外发布前）；L6 多模态排期。
- 40：多模态解析与许可证核准依据。
- 60：T2 增加音视频、宿主机解码、AIGC 标识、开源许可页的场景；owner 验收清单增加对应步骤。
- 70、80：新卡归属板块；AIGC 标识纳入确定性证明。
