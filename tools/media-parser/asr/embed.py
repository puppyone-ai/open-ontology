"""Speaker embedder: 3D-Speaker CAM++ as ONNX (Apache-2.0), 80-bin Kaldi fbank from kaldi-native-fbank (Apache-2.0).

Features: povey window, 25 ms / 10 ms, preemphasis 0.97, energy floor 1.0, DC offset removed, no dither, waveform in
int16 scale, then the per-window mean over time is subtracted. Output: a 192-dimension vector. The vector is used
for clustering and thrown away with the job (asr/diarize.py `EmbeddingStore.delete`); it is not stored anywhere else.
"""
import numpy as np


def fbank80(x16k):
    import kaldi_native_fbank as knf

    opts = knf.FbankOptions()
    opts.frame_opts.samp_freq = 16000
    opts.frame_opts.dither = 0.0
    opts.frame_opts.window_type = "povey"
    opts.frame_opts.frame_length_ms = 25.0
    opts.frame_opts.frame_shift_ms = 10.0
    opts.frame_opts.preemph_coeff = 0.97
    opts.frame_opts.remove_dc_offset = True
    opts.frame_opts.snip_edges = True
    opts.energy_floor = 1.0
    opts.mel_opts.num_bins = 80
    fb = knf.OnlineFbank(opts)
    fb.accept_waveform(16000, (np.asarray(x16k, dtype=np.float32) * 32768.0).tolist())
    fb.input_finished()
    n = fb.num_frames_ready
    if n == 0:
        return np.zeros((0, 80), dtype=np.float32)
    mat = np.stack([fb.get_frame(i) for i in range(n)]).astype(np.float32)
    return mat - mat.mean(axis=0, keepdims=True)


class CampPlusOnnx:
    def __init__(self, path, threads=1):
        import onnxruntime as ort

        so = ort.SessionOptions()
        so.intra_op_num_threads = threads
        so.inter_op_num_threads = 1
        self.sess = ort.InferenceSession(str(path), so, providers=["CPUExecutionProvider"])

    def embed(self, x16k):
        f = fbank80(x16k)
        if len(f) < 10:
            f = np.pad(f, ((0, 10 - len(f)), (0, 0)))
        out = self.sess.run(None, {"feats": f[None]})[0]
        return out[0]

    def embed_many(self, xs):
        return np.stack([self.embed(x) for x in xs])
