"""Speaker separation without a voiceprint store (D63, D73, D74).

* Every speech chunk is cut into 2 s windows (hop 1 s). The embedder (3D-Speaker through sherpa-onnx, or any
  object with `embed(samples, sr) -> vector`) turns each into a vector; the vectors are written to the job
  scratch (`EmbeddingStore`) so a resumed job does not recompute them.
* After all chunks are done, one agglomerative clustering (average linkage, cosine, numpy only) runs over the whole file.
* Exclusive output: every word goes to one speaker, the one of the window nearest to the word's midpoint, then a
  3-word median smoothing removes single-word flips.
* Labels are `S1`, `S2`, ... in order of first speech. They are valid inside this file only.
* `EmbeddingStore.delete()` removes every feature file when the file is finished. Nothing is kept in the CAS,
  the database or the IR.
"""
import shutil
from pathlib import Path

import numpy as np

WINDOW_MS = 2000
HOP_MS = 1000
MIN_WINDOW_MS = 800


def windows_for(start_ms, end_ms, window_ms=WINDOW_MS, hop_ms=HOP_MS, min_ms=MIN_WINDOW_MS):
    """Windows inside a chunk. A chunk shorter than `window_ms` gives one window (if at least `min_ms`)."""
    n = end_ms - start_ms
    if n < min_ms:
        return []
    if n <= window_ms:
        return [(start_ms, end_ms)]
    out, s = [], start_ms
    while s + window_ms <= end_ms:
        out.append((s, s + window_ms))
        s += hop_ms
    if out and out[-1][1] < end_ms and end_ms - out[-1][1] >= min_ms // 2:
        out.append((end_ms - window_ms, end_ms))
    return out


class EmbeddingStore:
    """Feature files of one job, one `.f32` per chunk, under `<job>/emb`."""

    def __init__(self, root):
        self.root = Path(root)

    def path(self, key):
        return self.root / f"{key}.f32"

    def has(self, key):
        return self.path(key).is_file()

    def put(self, key, matrix):
        self.root.mkdir(parents=True, exist_ok=True)
        m = np.asarray(matrix, dtype=np.float32)
        tmp = self.root / f".{key}.tmp"
        with open(tmp, "wb") as f:
            np.asarray(m.shape, dtype=np.int32).tofile(f)
            m.tofile(f)
        tmp.replace(self.path(key))

    def get(self, key):
        with open(self.path(key), "rb") as f:
            shape = np.fromfile(f, dtype=np.int32, count=2)
            return np.fromfile(f, dtype=np.float32).reshape(tuple(shape))

    def delete(self):
        shutil.rmtree(self.root, ignore_errors=True)

    def files(self):
        return sorted(p.name for p in self.root.glob("*")) if self.root.is_dir() else []


def cluster(vectors, threshold=0.55, num_speakers=None, run_similarity=0.9):
    """Cluster labels `0..k-1` for `vectors` (n, d) given in time order.

    Stage 1 joins neighbouring windows whose cosine similarity to the running mean is at least
    `run_similarity` (a speaker holds the floor for a while), so stage 2 works on a few hundred runs, not on
    thousands of windows. Stage 2 is average-linkage agglomerative clustering on cosine distance (Lance-Williams
    update, weighted by run size) with numpy only. It stops at `threshold` (cosine distance), or at
    `num_speakers` clusters when a meeting-metadata hint gives the number. Deterministic: ties go to the lowest index.
    """
    v = np.asarray(vectors, dtype=np.float64)
    n = len(v)
    if n == 0:
        return np.zeros(0, dtype=int)
    v = v / np.maximum(np.linalg.norm(v, axis=1, keepdims=True), 1e-9)
    run_of, sums, counts = [], [], []
    for x in v:
        if sums:
            m = sums[-1] / max(np.linalg.norm(sums[-1]), 1e-9)
            if float(m @ x) >= run_similarity:
                sums[-1] = sums[-1] + x
                counts[-1] += 1
                run_of.append(len(sums) - 1)
                continue
        sums.append(x.copy())
        counts.append(1)
        run_of.append(len(sums) - 1)
    cent = np.stack(sums)
    cent = cent / np.maximum(np.linalg.norm(cent, axis=1, keepdims=True), 1e-9)
    m = len(cent)
    d = 1.0 - cent @ cent.T
    np.fill_diagonal(d, np.inf)
    size = np.asarray(counts, dtype=np.float64)
    members = [[i] for i in range(m)]
    active = list(range(m))
    while len(active) > 1:
        sub = d[np.ix_(active, active)]
        flat = int(np.argmin(sub))
        i, j = divmod(flat, len(active))
        dist = sub[i, j]
        if num_speakers is None and dist > threshold:
            break
        if num_speakers is not None and len(active) <= num_speakers:
            break
        a, b = active[min(i, j)], active[max(i, j)]
        wa, wb = size[a], size[b]
        d[a, :] = (wa * d[a, :] + wb * d[b, :]) / (wa + wb)
        d[:, a] = d[a, :]
        d[a, a] = np.inf
        d[b, :] = np.inf
        d[:, b] = np.inf
        size[a] = wa + wb
        members[a] += members[b]
        active.remove(b)
    label_of_run = {}
    for lab, a in enumerate(active):
        for r in members[a]:
            label_of_run[r] = lab
    return np.asarray([label_of_run[r] for r in run_of], dtype=int)


SPECTRAL_SIGMA = 0.25
SPECTRAL_KMAX = 5
STRAY_SHARE = 0.15
STRAY_WINDOWS = 2
SPECTRAL_MIN_WINDOWS = 6
# One voice: its windows are alike (mean pairwise cosine 0.75-0.78 on single-voice development files, at most 0.63 with two voices).
SINGLE_VOICE_SIMILARITY = 0.70


def _spectral_count(c, sigma=SPECTRAL_SIGMA, kmax=SPECTRAL_KMAX):
    """Number of speakers from the eigengap of the normalised Laplacian of a cosine affinity (1..kmax)."""
    n = len(c)
    cn = c / np.maximum(np.linalg.norm(c, axis=1, keepdims=True), 1e-9)
    a = np.exp(-(1.0 - cn @ cn.T) / sigma)
    np.fill_diagonal(a, 0.0)
    d = np.maximum(a.sum(axis=1), 1e-12)
    lap = np.eye(n) - a / np.sqrt(np.outer(d, d))
    w = np.sort(np.linalg.eigvalsh(lap))[: kmax + 1]
    return int(np.argmax(np.diff(w)[:kmax])) + 1


def _merge_strays(labels, c, share=STRAY_SHARE, windows=STRAY_WINDOWS):
    """A cluster of at most `windows` windows and under `share` of them all is a clustering stray, not a speaker: it joins the nearest cluster."""
    labels = np.asarray(labels).copy()
    cn = c / np.maximum(np.linalg.norm(c, axis=1, keepdims=True), 1e-9)
    n = len(labels)
    while True:
        counts = {k: int((labels == k).sum()) for k in sorted(set(labels.tolist()))}
        small = [k for k, m in counts.items() if m < share * n and m <= windows]
        if not small or len(counts) == 1:
            return labels
        k = min(small, key=lambda x: (counts[x], x))
        cents = {j: cn[labels == j].mean(axis=0) for j in counts if j != k}
        mine = cn[labels == k].mean(axis=0)
        near = max(sorted(cents), key=lambda j: float(mine @ cents[j]) / (np.linalg.norm(mine) * np.linalg.norm(cents[j]) + 1e-9))
        labels[labels == k] = near


def cluster_auto(vectors, threshold=0.55):
    """Cluster labels without a speaker count (D98 re-tuned, lane AL).

    The embeddings are centred on their mean over the file, which removes what every voice shares (the synthesis or the channel) and lets
    the speakers stand apart; the count is the eigengap of a spectral graph (at most `SPECTRAL_KMAX`), the clusters come from the same
    agglomerative step as before, and a cluster of at most `STRAY_WINDOWS` windows (under `STRAY_SHARE` of them) is merged into its nearest. A file with fewer than
    `SPECTRAL_MIN_WINDOWS` windows keeps the plain threshold clustering. Deterministic. Tuned on synthetic development meetings
    (`tools/super-ontology/media_class/tune_clustering.py`).
    """
    v = np.asarray(vectors, dtype=np.float64)
    if len(v) < SPECTRAL_MIN_WINDOWS:
        return cluster(v, threshold)
    vn = v / np.maximum(np.linalg.norm(v, axis=1, keepdims=True), 1e-9)
    sim = vn @ vn.T
    if float((sim.sum() - len(v)) / (len(v) * (len(v) - 1))) >= SINGLE_VOICE_SIMILARITY:
        return np.zeros(len(v), dtype=int)
    c = v - v.mean(axis=0, keepdims=True)
    k = _spectral_count(c)
    labels = np.zeros(len(v), dtype=int) if k == 1 else cluster(c, 9.9, num_speakers=k)
    return _merge_strays(labels, c)


def relabel_by_first_appearance(labels, order_keys):
    """Map raw cluster ids to `S1..Sk` by the earliest `order_keys` (start time) of their members."""
    first = {}
    for lab, key in zip(labels, order_keys):
        if lab not in first or key < first[lab]:
            first[lab] = key
    ranking = {lab: f"S{i + 1}" for i, (lab, _) in enumerate(sorted(first.items(), key=lambda kv: (kv[1], int(kv[0]))))}
    return [ranking[lab] for lab in labels]


def assign_words(words, win_spans, win_labels):
    """Speaker label per word: nearest window centre, then a 3-word median smoothing.

    `words`: dicts with start_ms and end_ms in the same time base as `win_spans`.
    """
    if not win_spans:
        return ["S1"] * len(words)
    centres = np.asarray([(s + e) / 2 for s, e in win_spans], dtype=np.float64)
    order = np.argsort(centres, kind="stable")
    centres_sorted = centres[order]
    labels_sorted = [win_labels[i] for i in order]
    out = []
    for w in words:
        mid = (w["start_ms"] + w["end_ms"]) / 2
        k = int(np.searchsorted(centres_sorted, mid))
        cand = [j for j in (k - 1, k) if 0 <= j < len(centres_sorted)]
        j = min(cand, key=lambda j: (abs(centres_sorted[j] - mid), j))
        out.append(labels_sorted[j])
    smoothed = list(out)
    for i in range(1, len(out) - 1):
        if out[i - 1] == out[i + 1] != out[i]:
            smoothed[i] = out[i - 1]
    return smoothed
