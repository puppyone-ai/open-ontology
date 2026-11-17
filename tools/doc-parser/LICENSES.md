# doc-parser license investigation (B-S1-07)

Status: investigation for the `doc-parser` sidecar (spec 10 section 2.6, spec 00 D69, spec 90 section 2.1). Date 2026-11-17. This file lists what was confirmed, what was not, and what blocks the default image. It is not a lock file and it admits nothing: the sidecar image counts as a default artifact (policy section 1), so every Python package and model file below still needs its own record when `requirements.lock` and `models.lock.json` are written (B-S3-01).

## 1. Method and limits

- Docling 2.150.0 was not installed and nothing was executed. Evidence is metadata and text read over HTTPS: the PyPI JSON API; wheel `.metadata` files; individual license files read from inside wheels with HTTP range requests (the zip central directory and the member, nothing extracted to disk); the Hugging Face and ModelScope model APIs; GitHub raw files at tag `v2.150.0` (Docling) and `v3.10.0` (RapidOCR).
- The dependency closure was resolved, not installed, with `uv pip compile` for CPython 3.12, `linux/amd64` and `linux/arm64`, with `--exclude-newer 2026-11-10T10:29:30Z` (7*86400 seconds before the resolve, the same floor as `supply-chain/policy.md` section 4), `--generate-hashes`, and the PyTorch CPU index so that no `nvidia-*` / CUDA wheel enters the graph. `deploy/lite/images.lock.json` pins the Python base image by digest only, so the interpreter minor version is an assumption (3.12) to confirm.
- PyPI license fields are a hint. Where the wheel ships its own license files they were read; where it bundles native libraries the bundled-library licenses were read from the wheel. Anything not read is listed as unconfirmed below.
- Resolver outputs (with hashes) are kept next to the coordinator patches as `B-S1-07-docparser-requirements-cp312-{amd64,arm64}.candidate.txt` and the input as `B-S1-07-docparser-requirements.in`. They are candidates for B-S3-01, not a lock.

## 2. Install profile

`docling[rapidocr]` (what the spec text names) now resolves to 106 packages because Docling 2.150 split into `docling-slim` plus extras, and `docling[...]` pulls `docling-slim[standard]`, which adds mail-parser, polyfactory/faker, marko, beautifulsoup4, openpyxl, python-oxmsg, typer, websockets and tree-sitter. The narrower profile below resolves to 85 packages with an identical set on both architectures:

```
docling-slim[convert-core,format-pdf,format-docx,format-pptx,models-local,feat-ocr-rapidocr-onnx]==2.150.0
```

`docling-slim` 2.150.0 was uploaded 2026-11-10T07:58:50Z and so clears seven days only from 2026-11-17T07:58:50Z; it passes at this file's date. Not installed by this profile: `feat-ocr-nemotron` (NVIDIA Open Model License, banned by spec 90), `feat-ocr-easyocr` (LGPL python-bidi, weights unconfirmed), `feat-ocr-tesserocr`, `format-email` (mail-parser; its Outlook extra pulls extract-msg, GPL-3.0, which spec 90 section 3 says the lock must prove absent; absent here), `format-video`, `format-audio`, `models-vlm-inline`, `models-remote`. Whether `docling-slim` alone provides the `docling` import (library mode, no `docling-serve`) is to be confirmed by B-S3-01.

OCR is selected explicitly: `RapidOcrOptions(backend="onnxruntime")`, never `OcrAutoOptions` (which is Docling's default and may pick another engine). Docling's `do_ocr` defaults to true; picture classification, picture description, code and formula enrichment default to false and must stay false (they would download further models whose licenses are not cleared).

## 3. Model weights

| Model | Source and revision | License (as read 2026-11-17) | Used by | Status |
|---|---|---|---|---|
| docling-layout-heron | HF `docling-project/docling-layout-heron`, commit `8f39ad3c0b4c58e9c2d2c84a38465abf757272d8` (Docling's default layout model; Docling downloads revision `main`, pin the commit) | Apache-2.0 (HF `license:apache-2.0`) | default PDF layout | Confirmed from model card metadata; no LICENSE file in the repo (6 files), so the Apache text and NOTICE must be supplied by us |
| docling-models (TableFormer, `accurate` and `fast`) | HF `docling-project/docling-models`, commit `2199320848bb9a8a519d22e4b528185a4f9a6f64` | tags `cdla-permissive-2.0` and `apache-2.0` together | default table structure (`do_table_structure` is true) | UNCONFIRMED: which license governs is lawyer question L5 (spec 90 section 8). Ship both texts meanwhile |
| RapidOCR ONNX models (PP-OCRv6 det/rec small, PP-OCR cls) | ModelScope `RapidAI/RapidOCR`, tag `v3.10.0` (URLs come from `rapidocr` 3.10.0 `default_models.yaml`; Docling resolves them at first use unless `artifacts_path` is set) | ModelScope repo metadata "Apache License 2.0"; upstream PaddleOCR repo Apache-2.0; HF `PaddlePaddle/PP-OCRv6_*` Apache-2.0 | OCR | Repo-level license confirmed; per-file license is not stated on ModelScope. The download host is modelscope.cn: build-time fetch only, never at runtime |
| DocumentFigureClassifier-v2.5 | HF `docling-project/DocumentFigureClassifier-v2.5`, commit `f859dfbff5c9916cd996942d4b0db7fa25808220` | MIT | only if picture classification is on | Not used; keep off |

RapidOCR files for the default configuration (language `ch`, `onnxruntime`, size `small`), sha256 from the ModelScope file tree at tag `v3.10.0`:

| File | Size | sha256 |
|---|---|---|
| `onnx/PP-OCRv6/det/PP-OCRv6_det_small.onnx` | 9929594 | `090f04abcd9d9a7498bc4ebf677e4cb9bdce1fe4197ddb7e529f1ef44e1ff94f` |
| `onnx/PP-OCRv6/rec/PP-OCRv6_rec_small.onnx` | 21234383 | `6f327246b50388f3c176ae304bd95767ea6dc0c9ae92153ef8cbe210b3c14884` |
| `onnx/PP-OCRv4/cls/ch_ppocr_mobile_v2.0_cls_mobile.onnx` | 585532 | `e47acedf663230f8863ff1ab0e64dd2d82b838fceb5957146dab185a89d6215c` |

These are inputs to `models.lock.json`. The exact file set follows from the language and size chosen in the adapter, and the recognizer may need a dictionary file as well (`ppocrv6_dict.txt`, sha256 `b5f2bfe2bdd9448429e3e82b51c789775d9b42f2403d082b00662eb77e401c5d`, exists in the same tree); B-S3-01 must recompute the set from the adapter configuration.

Runtime rule: `HF_HUB_OFFLINE=1`, `TRANSFORMERS_OFFLINE=1`, `artifacts_path` set to the pre-fetched directory so RapidOCR never downloads. `docling-tools models download` is the build-time fetcher; run it only for `layout`, `tableformer` and `rapidocr`.

## 4. Python packages (85, CPython 3.12, linux amd64 and arm64)

`License` is the PyPI `license_expression`, or the `License` field/classifier when no expression exists. `Uploaded` is the earliest file upload date on PyPI. A note is present only where something needs action.

| Package | Version | License (PyPI) | Uploaded | Note |
|---|---|---|---|---|
| accelerate | 1.15.0 | Apache | 2026-09-09 |  |
| annotated-doc | 0.0.5 | MIT | 2026-07-28 |  |
| annotated-types | 0.8.0 | MIT | 2026-07-23 |  |
| anyio | 4.16.0 | MIT | 2026-11-01 |  |
| attrs | 26.1.0 | MIT | 2026-03-19 |  |
| certifi | 2026.10.23 | MPL-2.0 | 2026-10-23 | REVIEW: MPL-2.0 (weak copyleft) |
| charset-normalizer | 3.5.2 | MIT | 2026-09-30 |  |
| click | 8.5.1 | BSD-3-Clause | 2026-11-06 |  |
| colorlog | 6.12.0 | MIT License | 2026-07-23 |  |
| defusedxml | 0.7.1 | PSFL | 2021-03-08 | id not in governance list: PSF-2.0 |
| doclang | 0.9.0 | Apache-2.0 | 2026-10-30 |  |
| docling-core | 2.106.0 | MIT | 2026-11-04 |  |
| docling-ibm-models | 4.0.4 | MIT | 2026-10-21 |  |
| docling-parse | 7.25.0 | MIT | 2026-11-09 | ships pdf_resources/cmap-resources (separate LICENSE.md) |
| docling-slim | 2.150.0 | MIT | 2026-11-10 |  |
| filelock | 4.2.10 | MIT | 2026-11-09 |  |
| filetype | 1.2.0 | MIT | 2022-11-02 |  |
| flatbuffers | 25.12.19 | Apache 2.0 | 2025-12-19 |  |
| fsspec | 2026.9.0 | BSD-3-Clause | 2026-09-18 |  |
| h11 | 0.16.0 | MIT | 2025-04-24 |  |
| hf-xet | 1.8.0 | Apache-2.0 | 2026-10-30 |  |
| httpcore | 1.0.9 | BSD-3-Clause | 2025-04-24 |  |
| httpx | 0.28.1 | BSD-3-Clause | 2024-12-06 |  |
| huggingface-hub | 1.33.0 | Apache-2.0 | 2026-09-24 |  |
| idna | 3.20 | BSD-3-Clause | 2026-09-17 |  |
| jinja2 | 3.1.6 | BSD License | 2025-03-05 |  |
| jsonref | 1.1.0 | MIT | 2023-01-16 |  |
| jsonschema | 4.26.0 | MIT | 2026-01-07 |  |
| jsonschema-specifications | 2025.9.1 | MIT | 2025-09-08 |  |
| langcodes | 3.5.1 | MIT License | 2025-12-02 |  |
| latex2mathml | 3.81.1 | MIT | 2026-09-07 |  |
| lxml | 6.1.3 | BSD-3-Clause | 2026-09-02 | BLOCKER-REVIEW: LICENSES.txt lists iconv LGPL-2.1 (linked into the wheel, no separate .libs) |
| markdown-it-py | 4.2.0 | MIT License | 2026-05-07 |  |
| markupsafe | 3.0.4 | BSD-3-Clause | 2026-10-02 |  |
| mdurl | 0.1.2 | MIT License | 2022-08-14 |  |
| mpmath | 1.3.0 | BSD | 2023-03-07 |  |
| networkx | 3.7 | BSD-3-Clause | 2026-09-21 |  |
| numpy | 2.5.4 | BSD-3-Clause AND 0BSD AND MIT AND Zlib AND CC0-1.0 | 2026-10-10 | wheel bundles libgfortran (GPL-3.0-or-later WITH GCC-exception-3.1) and libquadmath (LGPL-2.1-or-later) |
| omegaconf | 2.4.0 | BSD License | 2026-10-09 |  |
| onnxruntime | 1.31.1 | MIT License | 2026-10-26 | ships ThirdPartyNotices.txt; contents to be read at SBOM time |
| opencv-python-headless | 5.0.0.93 | Apache 2.0 | 2026-07-02 | BLOCKER: wheel bundles FFmpeg libs (libavcodec, libavformat, ...), LGPL; conflicts with D65 |
| packaging | 26.3 | Apache-2.0 OR BSD-2-Clause | 2026-08-04 |  |
| pandas | 3.1.1 | BSD-3-Clause | 2026-11-04 | 19 bundled third-party license files (BSD/MIT family) |
| pillow | 13.0.0 | MIT-CMU | 2026-10-16 | id not in governance list: MIT-CMU; wheel bundles libavif, freetype, harfbuzz, jpeg, lcms2, openjp2, png, tiff, webp, xcb, zstd, lzma, brotli: notices to confirm |
| pluggy | 1.7.0 | MIT | 2026-10-15 |  |
| protobuf | 7.36.2 | 3-Clause BSD License | 2026-09-17 |  |
| psutil | 7.2.2 | BSD-3-Clause | 2026-01-28 |  |
| pyclipper | 1.4.0 | MIT | 2025-12-01 |  |
| pydantic | 2.14.3 | MIT | 2026-11-05 |  |
| pydantic-core | 2.50.4 | MIT | 2026-11-05 |  |
| pydantic-settings | 2.16.2 | MIT | 2026-10-28 |  |
| pygments | 2.21.0 | BSD-2-Clause | 2026-08-17 |  |
| pylatexenc | 2.11 | MIT | 2026-07-25 |  |
| pypdfium2 | 5.15.2 | BSD-3-Clause, Apache-2.0, dependency licenses | 2026-11-09 | 37 license files in the wheel (Apache-2.0, BSD-3-Clause, CC-BY-4.0, PDFium build licenses); all must ship |
| python-dateutil | 4.0.0 | BSD-3-Clause | 2026-11-01 |  |
| python-docx | 1.2.0 | MIT | 2025-06-16 |  |
| python-dotenv | 1.2.4 | BSD-3-Clause | 2026-10-01 |  |
| python-pptx | 1.0.2 | MIT | 2024-08-07 |  |
| pyyaml | 6.0.3 | MIT | 2025-09-25 |  |
| rapidocr | 3.10.0 | Apache-2.0 | 2026-10-08 |  |
| referencing | 0.37.0 | MIT | 2025-10-13 |  |
| regex | 2026.11.9 | Apache-2.0 AND CNRI-Python | 2026-11-09 | id not in governance list: CNRI-Python (with Apache-2.0) |
| requests | 2.34.2 | Apache-2.0 | 2026-05-14 |  |
| rich | 15.0.0 | MIT | 2026-04-12 |  |
| rpds-py | 2026.9.1 | MIT | 2026-10-04 |  |
| rtree | 1.4.1 | MIT | 2025-08-13 | wheel bundles libspatialindex (MIT) |
| safetensors | 0.8.0 | Apache Software License | 2026-06-09 |  |
| scipy | 1.18.1 | BSD-3-Clause (from the LICENSE text) | 2026-08-21 | wheel bundles libgfortran (GPL-3.0-or-later WITH GCC-exception-3.1) and libquadmath (LGPL-2.1-or-later); LICENSE.txt lists both |
| setuptools | 84.2.1 | MIT | 2026-10-20 |  |
| shapely | 2.2.0 | BSD-3-Clause | 2026-10-07 | BLOCKER-REVIEW: wheel bundles GEOS (LGPL-2.1) as shared libs; LICENSE_GEOS present |
| shellingham | 1.5.4 | ISC License | 2023-10-24 |  |
| six | 1.17.0 | MIT | 2024-12-04 |  |
| sympy | 1.14.0 | BSD | 2025-04-27 |  |
| tabulate | 0.10.0 | MIT | 2026-03-04 |  |
| tokenizers | 0.23.3 | Apache Software License | 2026-10-09 |  |
| torch | 2.15.0+cpu | Apache-2.0 AND Apache-2.0 WITH LLVM-exception AND BSD-2-Clause AND BSD-3-Clause AND BSL-1.0 AND ISC AND MIT AND Zlib | 2026-10-28 | wheel bundles torch/lib/libgomp.so.1 (GCC libgomp, GPL-3.0-or-later WITH GCC-exception-3.1); not in the wheel License-Expression |
| torchvision | 0.29.2+cpu | BSD | 2026-10-28 | check bundled libs with torch (same CPU index) |
| tqdm | 4.70.1 | MPL-2.0 AND MIT | 2026-09-11 | REVIEW: MPL-2.0 AND MIT (MPL-2.0 applies to a few files) |
| transformers | 5.22.2 | Apache 2.0 License | 2026-11-06 |  |
| typer | 0.27.3 | MIT | 2026-10-06 |  |
| typing-extensions | 4.16.0 | PSF-2.0 | 2026-07-02 | id not in governance list: PSF-2.0 |
| typing-inspection | 0.4.5 | MIT | 2026-11-04 |  |
| tzdata | 2026.6 | Apache-2.0 | 2026-10-28 |  |
| urllib3 | 2.8.0 | MIT | 2026-09-15 |  |
| xlsxwriter | 3.3.1 | BSD-2-Clause | 2026-10-28 |  |

## 5. Findings

Blockers for the default image (each needs an owner decision or a rebuild before B-S3-01 can close):

1. **OpenCV bundles FFmpeg.** `opencv-python-headless` 5.0.0.93 is a hard dependency of `rapidocr` and ships libavcodec, libavformat and others (LGPL; its `LICENSE-3RD-PARTY.txt` says FFmpeg is redistributed in all opencv-python packages). D65 allows only a self-built OpenCV without FFmpeg. Options: build OpenCV from source in the image builder with FFmpeg off and install it under the same distribution name and version, or provide an equivalent wheel. Unconfirmed: that a source build satisfies pip's requirement for `opencv-python-headless>=4.5.1.48`.
2. **LGPL components not in spec 90 section 3.** GEOS in `shapely` (LGPL-2.1, shared libraries, replaceable); iconv in `lxml` (LGPL-2.1 per its `LICENSES.txt`); `libquadmath` (LGPL-2.1-or-later) and `libgfortran` (GPL-3.0-or-later with GCC runtime exception) in `numpy` and `scipy`; `libgomp` (GPL-3.0-or-later with GCC runtime exception) in the `torch` CPU wheel. Policy section 2 requires a written weak-copyleft review for each LGPL component (reviewer role, obligation, evidence, scope). None exists yet. Spec 90 L7 already asks the lawyer about LGPL distribution in images; these belong in that question. `shapely` and the OpenCV problem both arrive through `rapidocr`, so a smaller OCR stack (or a vendored ONNX-only path) would remove them; not evaluated.
3. **MPL-2.0 packages.** `certifi` (CA bundle data) and `tqdm` (MPL-2.0 for a few files) are weak copyleft and need the same written review. Both are unmodified; the obligation is source availability for those files.
4. **TableFormer license ambiguity** (L5, open).
5. **No NOTICE or SBOM yet.** Native libraries inside wheels (Pillow's libavif, freetype, harfbuzz, tiff, webp, zstd and others; rtree's libspatialindex; onnxruntime's `ThirdPartyNotices.txt`; pypdfium2's 37 files) are known only by name. The image-level SBOM gate (spec 90 section 3, D64) is the check that settles them; until it runs they are unconfirmed.

Not blockers but decisions:

- `PSF-2.0` (defusedxml, typing-extensions), `CNRI-Python` (regex) and `MIT-CMU` (Pillow) are permissive licenses that are not in the identifier list of `governance/qa_license.py`, so a record for them is classed "unknown" by that checker until the list is extended. Extending it is a coordinator decision.
- Docling pins: `docling-slim` 2.150.0, `docling-core` 2.106.0, `docling-ibm-models` 4.0.4, `docling-parse` 7.25.0 (all MIT). Newer releases exist (docling 2.150.1, docling-core 2.107.1, docling-parse 7.26.0, onnxruntime 1.32.0) and were not admitted because they were younger than seven days at resolve time.
- `rapidocr` 3.10.0 is Apache-2.0; it also requires `pyclipper` (MIT), `shapely`, `opencv-python-headless`, `omegaconf`, `colorlog`.
- The Python base image and its OS packages are covered by the image SBOM gate, not by this file.

## 6. What this file does not establish

No wheel was installed or imported, so import-time behavior (for example whether any package contacts the network, or whether `docling-slim` exposes the `docling` module) is unverified. No legal conclusion is drawn; spec 90 section 8 questions L5 and L7 remain open and decide the TableFormer and LGPL points.

## 8. B-S3-01 outcome (L3 image, 2026-11-17)

- Profile built: `docling-slim[convert-core,format-pdf,format-docx,format-pptx,models-local]==2.150.0` on CPython 3.13 (the python digest in `deploy/lite/images.lock.json` is 3.13.18, not 3.12 as assumed in section 1). 75 packages, identical on linux aarch64 and x86_64, resolved by `tools/doc-parser/lock.py` into `requirements.lock` (hashes of both architectures, `--require-hashes`). CPU-only torch from the PyTorch CPU index; no `nvidia-*`.
- Not in the closure, proven by the image SBOM (`testing/super-ontology/evidence/L3/sbom-doc-parser.json`): rapidocr, easyocr, onnxruntime, shapely, opencv (any variant), FFmpeg libraries, PyMuPDF, extract-msg, soynlp. Sections 5.1 to 5.3 of this file (the OpenCV/FFmpeg and shapely blockers) therefore do not apply to the L3 image; they return with OCR in M2.
- **OpenCV is still wanted by one Docling component.** `docling-ibm-models` 4.0.4 (`tableformer/data_management/tf_predictor.py` and `functional.py`) imports `cv2` at module import, and `docling.models.stages.table_structure.table_structure_model.TableStructureModel.__init__` imports `TFPredictor`. `docling-ibm-models` does not declare an OpenCV requirement, so the resolver never installs one. Result: PDF table structure (TableFormer) cannot run in this image without an OpenCV wheel, and every official `opencv-python*` wheel bundles FFmpeg (D65). Decision for L3: `do_table_structure=False` for PDF, reported as `limits.table_structure=false` in `/v1/info` and as warning `DOCS_PARSE_TABLE_STRUCTURE_OFF`; docx and pptx tables are unaffected (python-docx, python-pptx). TableFormer weights are not shipped, which also removes lawyer question L5 from the L3 image. To enable it later: an OpenCV built without FFmpeg, installed under the distribution name `opencv-python-headless`, plus re-adding the `docling-models` source to `models.lock.json`.
- Layout model: `docling-project/docling-layout-heron` at `8f39ad3c0b4c58e9c2d2c84a38465abf757272d8`, 6 files, sha256 in `models.lock.json`; the Apache-2.0 text and NOTICE still have to be supplied (the repository has none).
- SBOM gate result and the five components that wait for a written weak-copyleft review are in `testing/super-ontology/evidence/L3/sbom-doc-parser.gate.txt`.

## 9. B-S3-01b outcome: OpenCV without FFmpeg, TableFormer on (2026-11-17)

- Section 8's last OpenCV bullet is resolved. The image builds `opencv-python-headless` 5.0.0.93 from the PyPI source distribution (URL and sha256 pinned in the `Dockerfile` stage `opencv`) with `-DWITH_FFMPEG=OFF -DWITH_GSTREAMER=OFF -DWITH_V4L=OFF`, every codec library off and only the modules the Python typing-stub generator needs (`core imgproc imgcodecs flann features geometry calib stereo objdetect`). videoio, video, dnn and highgui are not built, so `cv2` has no `VideoCapture` and no libav*/libsw* file exists in the image. Build tools are locked by hash in `opencv-build-requirements.lock` and live in the builder stage only.
- Why that module list and not `core,imgproc` alone: OpenCV's stub generator refines `Feature2D`, `imread`, calib and other symbols and aborts the wheel build when the modules that define them are missing.
- TableFormer (`docling-project/docling-models` at `2199320848bb9a8a519d22e4b528185a4f9a6f64`, `accurate` variant, the mode Docling uses by default) is shipped again. Its license: the model card and HF API list `cdla-permissive-2.0` and `apache-2.0` together and the repository has no license file (re-read 2026-11-17). Spec 90 section 2 admits it conditionally: both texts ship with the product, and counsel (L5) decides which governs. Both are permissive, so the conditions are met by shipping both texts, which `THIRD_PARTY_NOTICES.md` does. L5 stays open for counsel.
- SBOM: the `opencv_python_headless` component carries `oo:opencv_ffmpeg=absent` (read from `cv2.getBuildInformation()` inside the image); the gate fails an OpenCV distribution without that attestation or with `YES`.
