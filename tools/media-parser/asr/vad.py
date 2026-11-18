"""silero-vad (MIT, the ONNX model of the silero-vad 6.2.3 wheel) run directly with onnxruntime: no torch, no
silero-vad package. 16 kHz, 512-sample frames (32 ms) with a 64-sample context, one probability per frame.
The README badge of silero-vad says CC BY-NC while its LICENSE file is MIT; the LICENSE snapshot is kept in
THIRD_PARTY_NOTICES (90 section 2.2)."""
import numpy as np

FRAME = 512
CONTEXT = 64


class SileroVad:
    def __init__(self, path, threads=1):
        import onnxruntime as ort

        so = ort.SessionOptions()
        so.intra_op_num_threads = threads
        so.inter_op_num_threads = 1
        self.sess = ort.InferenceSession(str(path), so, providers=["CPUExecutionProvider"])

    def probs(self, x):
        x = np.asarray(x, dtype=np.float32)
        n = len(x) // FRAME
        state = np.zeros((2, 1, 128), dtype=np.float32)
        context = np.zeros(CONTEXT, dtype=np.float32)
        sr = np.array(16000, dtype=np.int64)
        out = []
        for i in range(n):
            frame = x[i * FRAME:(i + 1) * FRAME]
            inp = np.concatenate([context, frame])[None]
            p, state = self.sess.run(None, {"input": inp, "state": state, "sr": sr})
            out.append(float(p[0, 0]))
            context = frame[-CONTEXT:]
        return out
