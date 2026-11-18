#!/usr/bin/env python3
"""Export the 3D-Speaker CAM++ speaker embedding model (Apache-2.0, ModelScope `iic/speech_campplus_sv_zh-cn_16k-common`)
to ONNX, and check that the ONNX path (kaldi-native-fbank + onnxruntime) gives the same embeddings as the PyTorch one.

    python3 export_campplus_onnx.py MODEL_DIR OUT.onnx [WAV]

Builder tool: needs torch, torchaudio and modelscope, none of which are in the media-parser image. The image carries
only the resulting ONNX file, pinned by sha256 in models.lock.json. The embedding of a window is computed from an
80-bin Kaldi fbank with the per-utterance mean removed (what the ModelScope pipeline does), see `asr/embed.py`.
"""
import hashlib
import sys
from pathlib import Path


def export(model_dir, out):
    import torch
    from modelscope.pipelines import pipeline
    from modelscope.utils.constant import Tasks

    sv = pipeline(task=Tasks.speaker_verification, model=str(model_dir))
    net = sv.model.embedding_model.eval()
    dummy = torch.randn(1, 200, 80)
    torch.onnx.export(net, (dummy,), str(out), input_names=["feats"], output_names=["emb"],
                      dynamic_axes={"feats": {0: "b", 1: "t"}, "emb": {0: "b"}}, opset_version=17, dynamo=False)
    return sv


def main(argv):
    model_dir, out = argv[1], Path(argv[2])
    sv = export(model_dir, out)
    print("sha256", hashlib.sha256(out.read_bytes()).hexdigest(), "bytes", out.stat().st_size)
    if len(argv) > 3:
        import wave

        import numpy as np

        sys.path.insert(0, str(Path(__file__).resolve().parent))
        from asr.embed import CampPlusOnnx

        w = wave.open(argv[3])
        x = np.frombuffer(w.readframes(w.getnframes()), dtype="<i2").astype(np.float32) / 32768
        segs = [x[i:i + 32000] for i in range(0, len(x) - 32000, 16000)][:12]
        ref = np.asarray(sv(segs + [segs[0]], output_emb=True)["embs"][:len(segs)])
        got = CampPlusOnnx(out).embed_many(segs)
        cos = (ref * got).sum(1) / np.linalg.norm(ref, axis=1) / np.linalg.norm(got, axis=1)
        print("cosine torch vs onnx: min", float(cos.min()), "mean", float(cos.mean()))
        assert cos.min() > 0.99


if __name__ == "__main__":
    main(sys.argv)
