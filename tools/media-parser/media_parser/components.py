"""Components of the media-parser image and their admission verdicts (90 section 2.2). The SBOM gate compares
this list with the image SBOM; `forbidden` names must never be present."""

COMPONENTS = [
    {"name": "ffmpeg", "spdx": "LGPL-2.1-or-later", "note": "own build, decode only, no h264/hevc/aac, dynamic"},
    {"name": "silero-vad", "spdx": "MIT", "note": "ONNX model only, run with onnxruntime"},
    {"name": "sherpa-onnx", "spdx": "Apache-2.0"},
    {"name": "3D-Speaker", "spdx": "Apache-2.0", "note": "speaker embedding ONNX; features are discarded after use"},
    {"name": "PySceneDetect", "spdx": "BSD-3-Clause", "note": "0.7.3, with the FFmpeg-free OpenCV"},
    {"name": "imagehash", "spdx": "BSD-2-Clause"},
    {"name": "rapidocr", "spdx": "Apache-2.0", "note": "PP-OCRv5 ONNX"},
]

FORBIDDEN = ("qwen-asr", "soynlp", "nemotron-ocr", "diarizen", "SenseVoiceSmall", "pyannote.audio",
             "opencv-python", "opencv-python-headless", "opencv-contrib-python")
