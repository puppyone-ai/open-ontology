"""Real engines for the media-parser image: silero VAD, Paraformer-zh ONNX, CAM++ ONNX (bake-off result, D72)."""
import json
import os
from pathlib import Path

MODELS_DIR = os.environ.get("OO_MEDIA_MODELS", "/opt/models")
MODELS_LOCK = os.environ.get("OO_PARSER_MODELS_LOCK", "/opt/app/models.lock.json")


def lock_models():
    try:
        lock = json.loads(Path(MODELS_LOCK).read_text())
    except (OSError, ValueError):
        return []
    return [{"name": f["name"], "sha256": f["sha256"]} for f in lock.get("files", []) if f["name"].endswith((".onnx",))]


class CampEmbedder:
    def __init__(self, path):
        from asr.embed import CampPlusOnnx

        self.m = CampPlusOnnx(path)

    def embed(self, x):
        return self.m.embed(x)


def load(opts):
    from asr.paraformer_onnx import ParaformerOnnx
    from asr.vad import SileroVad

    d = Path(MODELS_DIR)
    return (SileroVad(d / "silero-vad" / "silero_vad.onnx"), ParaformerOnnx(d / "paraformer-zh"),
            CampEmbedder(d / "campplus" / "campplus.onnx"), lock_models())
